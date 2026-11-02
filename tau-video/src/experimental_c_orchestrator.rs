//! Opt-in experimental C-reference workflow from an already decoded Y4M stream.
//!
//! This vertical slice groups Y4M frames, runs bounded concurrent `tvc_enc` segment jobs,
//! repairs cross-segment rate windows with `-P`, merges TVC1 chunks, then muxes and checks a
//! video-only TNV3. It does not replace the Python workflow or invoke FFmpeg itself.
use crate::y4m::Y4mFrameGroups;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, JoinHandle},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const MAX_SEGMENT_FRAMES: usize = 480;
const MAX_JOBS: usize = 6;
const MAX_SEGMENTS: usize = 4096;
const DEFAULT_WORKSPACE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const TNV3_OVERHEAD_RESERVE: u64 = 32 * 1024 * 1024;
// Fits a complete 480-frame 400x360 YUV420 segment, including header and metadata.
const MAX_SEGMENT_Y4M_BYTES: u64 = 128 * 1024 * 1024;
// Checked after each C child exits; this is not an OS-enforced write quota.
const MAX_ACCEPTED_SEGMENT_TVC_BYTES: u64 = 16 * 1024 * 1024;
const LEVEL_WINDOW_BYTES: [u64; 4] = [0, 200_000, 400_000, 600_000];

struct BoundedWriter<W> {
    inner: W,
    limit: u64,
    written: u64,
}
impl<W> BoundedWriter<W> {
    fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            limit,
            written: 0,
        }
    }
}
impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let requested = u64::try_from(bytes.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "segment write length overflow")
        })?;
        let end = self.written.checked_add(requested).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "segment byte count overflow")
        })?;
        if end > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Y4M segment byte limit exceeded",
            ));
        }
        let count = self.inner.write(bytes)?;
        self.written = self.written.checked_add(count as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "segment byte count overflow")
        })?;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Executables used by this experimental workflow.
#[derive(Clone, Debug)]
pub struct Tools {
    pub tvc_encoder: PathBuf,
    pub tnv3: PathBuf,
}

/// Bounded settings for a pre-decoded Y4M stream.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub qp: u8,
    pub level: u8,
    pub frames_per_segment: usize,
    pub jobs: usize,
    /// Projected workspace budget (default 512 MiB), checked around child jobs and outputs.
    /// This is not a hard filesystem quota; a C child can exceed its reservation before exit.
    pub workspace_limit_bytes: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            qp: 30,
            // Zero selects the smallest TVC level that fits the actual Y4M header.
            level: 0,
            frames_per_segment: 120,
            jobs: 1,
            workspace_limit_bytes: DEFAULT_WORKSPACE_LIMIT_BYTES,
        }
    }
}

/// Summary of the checked video-only output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub frames: u32,
    pub level: u8,
    pub segments: u32,
    pub joint_reencodes: u32,
    pub worst_window_bytes: u64,
    pub tnv3_bytes: u64,
}

struct TempDir {
    path: PathBuf,
    armed: bool,
}
impl TempDir {
    fn new() -> io::Result<Self> {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "tau-c-orch-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&path) {
                Ok(()) => return Ok(Self { path, armed: true }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve orchestration workspace",
        ))
    }
    fn cleanup(mut self) -> io::Result<()> {
        fs::remove_dir_all(&self.path)?;
        self.armed = false;
        Ok(())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = fs::remove_dir_all(&self.path) {
                eprintln!(
                    "could not clean orchestration workspace {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}
#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}
#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}

/// Encode an already-decoded Y4M stream using the reference C encoder, then mux and validate it.
///
/// The writer is untouched until C encoding, rate-window repair, mux, and TNV3 validation all
/// succeed. An I/O failure while copying the final file can still leave partial output. A cleanup
/// error after copying can also be returned even though the writer received the complete checked
/// TNV3. This function consumes Y4M EOF; if a caller feeds it from FFmpeg stdout, that caller must
/// wait for FFmpeg and check its exit status before accepting the result. Keep `output` private
/// staged storage until that upstream status is checked; this function only sees stream EOF.
pub fn encode_y4m_video<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
) -> Result<Report, String> {
    validate_options(options)?;
    let temp = TempDir::new()
        .map_err(|error| format!("could not create private work directory: {error}"))?;
    let result = encode_in_workspace(tools, input, output, options, &temp.path);
    let cleanup = temp.cleanup();
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(format!(
            "encode succeeded but work directory cleanup failed: {cleanup}"
        )),
        (Err(error), Err(cleanup)) => Err(format!(
            "{error}; work directory cleanup also failed: {cleanup}"
        )),
    }
}

fn validate_options(options: Options) -> Result<(), String> {
    if options.qp > 51 {
        return Err("QP must be between 0 and 51".into());
    }
    if options.level > 3 {
        return Err("level must be 0 (auto), 1, 2, or 3".into());
    }
    if !(1..=MAX_SEGMENT_FRAMES).contains(&options.frames_per_segment) {
        return Err(format!(
            "frames_per_segment must be between 1 and {MAX_SEGMENT_FRAMES}"
        ));
    }
    if !(1..=MAX_JOBS).contains(&options.jobs) {
        return Err(format!("jobs must be between 1 and {MAX_JOBS}"));
    }
    if options.workspace_limit_bytes == 0 {
        return Err("workspace_limit_bytes must be positive".into());
    }
    Ok(())
}

/// Called only after a non-empty Y4M group has been read, so EOF at exactly `limit` is valid.
fn validate_next_segment(existing_segments: usize, limit: usize) -> Result<(), String> {
    if existing_segments >= limit {
        Err(format!("segment count exceeds {limit} bound"))
    } else {
        Ok(())
    }
}

struct PendingSegment {
    index: u32,
    frames: u32,
    y4m: PathBuf,
    tvc: PathBuf,
    worker: JoinHandle<Result<(), String>>,
}

#[derive(Default)]
struct MergeState {
    file: Option<File>,
    header: Option<Vec<u8>>,
    tail: VecDeque<u32>,
    frames: u64,
    segments: usize,
    joint_reencodes: u32,
    worst_window_bytes: u64,
}

#[derive(Clone, Copy)]
struct PipelineConfig {
    options: Options,
    level: u8,
    window: usize,
    limit: u64,
}

fn encode_in_workspace<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    work: &Path,
) -> Result<Report, String> {
    let mut groups = Y4mFrameGroups::new_c_compatible(input, options.frames_per_segment)
        .map_err(|error| format!("invalid Y4M stream: {error}"))?;
    let level = if options.level == 0 {
        (1..=3)
            .find(|&candidate| check_level_fit(groups.header(), candidate).is_ok())
            .ok_or("Y4M header does not fit any TVC level")?
    } else {
        check_level_fit(groups.header(), options.level)?;
        options.level
    };
    let max_group_bytes = groups
        .frame_bytes()
        .checked_mul(options.frames_per_segment as u64)
        .ok_or("maximum segment Y4M size overflow")?;
    if max_group_bytes > MAX_SEGMENT_Y4M_BYTES {
        return Err(format!(
            "segment Y4M bound exceeded: {max_group_bytes} > {MAX_SEGMENT_Y4M_BYTES} bytes"
        ));
    }
    let window = rate_window(groups.header())?;
    let limit = LEVEL_WINDOW_BYTES[level as usize];
    let config = PipelineConfig {
        options,
        level,
        window,
        limit,
    };
    let mut workers = VecDeque::new();
    let mut merged = MergeState::default();
    let mut next_segment = 0u32;
    let split_result = (|| -> Result<(), String> {
        loop {
            let y4m = work.join(format!("segment{next_segment:05}.y4m"));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&y4m)
                .map_err(|error| format!("could not create segment input: {error}"))?;
            let mut writer = BoundedWriter::new(BufWriter::new(file), MAX_SEGMENT_Y4M_BYTES);
            let count = match groups.write_next_group(&mut writer) {
                Ok(Some(count)) => count,
                Ok(None) => {
                    drop(writer);
                    fs::remove_file(&y4m).map_err(|error| {
                        format!("could not remove unused segment file: {error}")
                    })?;
                    break;
                }
                Err(error) => return Err(format!("Y4M grouping failed: {error}")),
            };
            writer
                .flush()
                .map_err(|error| format!("could not flush segment: {error}"))?;
            drop(writer);
            validate_next_segment(merged.segments + workers.len(), MAX_SEGMENTS)?;
            if groups.total_frames() > crate::MAX_FRAMES as u64 {
                return Err(format!(
                    "frame count exceeds TNV3 limit {}",
                    crate::MAX_FRAMES
                ));
            }

            let frames =
                u32::try_from(count).map_err(|_| "segment frame count overflow".to_string())?;
            let tvc = work.join(format!("segment{next_segment:05}.tvc"));
            let tvc_tool = tools.tvc_encoder.clone();
            let qp = options.qp;
            let worker_input = y4m.clone();
            let worker_output = tvc.clone();
            workers.push_back(PendingSegment {
                index: next_segment,
                frames,
                y4m,
                tvc,
                worker: thread::spawn(move || {
                    run_tvc(&tvc_tool, &worker_input, &worker_output, qp, level, None)
                }),
            });
            check_workspace_capacity(
                work,
                (workers.len() as u64)
                    .checked_mul(MAX_ACCEPTED_SEGMENT_TVC_BYTES)
                    .ok_or("workspace reserve overflow")?,
                options.workspace_limit_bytes,
            )?;
            next_segment = next_segment
                .checked_add(1)
                .ok_or_else(|| "segment count overflow".to_string())?;
            if workers.len() >= options.jobs {
                accept_next_segment(tools, work, &mut workers, &mut merged, config)?;
            }
        }
        while !workers.is_empty() {
            accept_next_segment(tools, work, &mut workers, &mut merged, config)?;
        }
        Ok(())
    })();
    let worker_result = join_remaining(&mut workers);
    split_result?;
    worker_result?;
    check_workspace_budget(work, options.workspace_limit_bytes)?;
    if merged.segments == 0 {
        return Err("Y4M stream contains no frames".into());
    }
    if merged.frames != groups.total_frames() {
        return Err(format!(
            "merged {} pictures; expected {}",
            merged.frames,
            groups.total_frames()
        ));
    }

    let merged_path = work.join("video.tvc");
    let mut merged_file = merged.file.take().ok_or("missing merged TVC writer")?;
    merged_file
        .seek(SeekFrom::Start(12))
        .map_err(|error| format!("could not seek to merged frame count: {error}"))?;
    merged_file
        .write_all(
            &u32::try_from(groups.total_frames())
                .map_err(|_| "frame count exceeds TVC header")?
                .to_le_bytes(),
        )
        .map_err(|error| format!("could not write merged frame count: {error}"))?;
    merged_file
        .seek(SeekFrom::End(0))
        .map_err(|error| format!("could not seek to merged TVC end: {error}"))?;
    merged_file
        .flush()
        .map_err(|error| format!("could not flush merged TVC: {error}"))?;
    drop(merged_file);

    check_workspace_capacity(
        work,
        fs::metadata(&merged_path)
            .map_err(|error| format!("could not inspect merged TVC: {error}"))?
            .len()
            .checked_add(TNV3_OVERHEAD_RESERVE)
            .ok_or("workspace reserve overflow")?,
        options.workspace_limit_bytes,
    )?;
    let output_path = work.join("output.tnv3");
    run_status(
        &tools.tnv3,
        &[
            "mux",
            merged_path.to_str().ok_or("non-UTF8 work path")?,
            "-c",
            "-o",
            output_path.to_str().ok_or("non-UTF8 work path")?,
        ],
        "tnv3 mux",
    )?;
    check_workspace_budget(work, options.workspace_limit_bytes)?;
    run_status(
        &tools.tnv3,
        &["check", output_path.to_str().ok_or("non-UTF8 work path")?],
        "tnv3 check",
    )?;
    let metadata = fs::metadata(&output_path)
        .map_err(|error| format!("could not inspect checked TNV3: {error}"))?;
    let mut encoded = File::open(&output_path)
        .map_err(|error| format!("could not open checked TNV3: {error}"))?;
    io::copy(&mut encoded, output)
        .map_err(|error| format!("could not copy checked TNV3 to output: {error}"))?;
    Ok(Report {
        frames: u32::try_from(groups.total_frames())
            .map_err(|_| "frame count overflow".to_string())?,
        level,
        segments: u32::try_from(merged.segments)
            .map_err(|_| "segment count overflow".to_string())?,
        joint_reencodes: merged.joint_reencodes,
        worst_window_bytes: merged.worst_window_bytes,
        tnv3_bytes: metadata.len(),
    })
}

fn accept_next_segment(
    tools: &Tools,
    work: &Path,
    workers: &mut VecDeque<PendingSegment>,
    merged: &mut MergeState,
    config: PipelineConfig,
) -> Result<(), String> {
    let segment = workers.pop_front().ok_or("missing segment worker")?;
    segment
        .worker
        .join()
        .map_err(|_| format!("segment {} worker panicked", segment.index))??;
    let mut parsed = parse_tvc(&segment.tvc)?;
    if parsed.lengths.len() != segment.frames as usize {
        return Err(format!(
            "segment {} frame count differs from Y4M",
            segment.index
        ));
    }

    let prefix = merged.tail.iter().copied().collect::<Vec<_>>();
    if segment.index > 0 && !window_ok(&prefix, &parsed.lengths, config.window, config.limit) {
        let previous = prefix
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        run_tvc(
            &tools.tvc_encoder,
            &segment.y4m,
            &segment.tvc,
            config.options.qp,
            config.level,
            Some(&previous),
        )?;
        parsed = parse_tvc(&segment.tvc)?;
        if parsed.lengths.len() != segment.frames as usize {
            return Err(format!(
                "re-encoded segment {} frame count changed",
                segment.index
            ));
        }
        merged.joint_reencodes = merged
            .joint_reencodes
            .checked_add(1)
            .ok_or_else(|| "joint retry counter overflow".to_string())?;
    }
    if !window_ok(&prefix, &parsed.lengths, config.window, config.limit) {
        let worst = worst_window_with_prefix(&prefix, &parsed.lengths, config.window);
        return Err(format!(
            "rate window remains over level budget: {worst} > {}",
            config.limit
        ));
    }

    let header = merged.header.get_or_insert_with(|| parsed.header.clone());
    if parsed.header.len() != 20
        || parsed.header[4..12] != header[4..12]
        || parsed.header[16..20] != header[16..20]
    {
        return Err(format!(
            "segment {} TVC header differs from prior segments",
            segment.index
        ));
    }
    if merged.file.is_none() {
        let path = work.join("video.tvc");
        let mut file = OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("could not create merged TVC: {error}"))?;
        file.write_all(header)
            .map_err(|error| format!("could not write merged TVC header: {error}"))?;
        merged.file = Some(file);
    }
    append_tvc_chunks(
        &parsed.bytes,
        &parsed.lengths,
        merged.file.as_mut().ok_or("missing merged TVC writer")?,
    )?;

    merged.worst_window_bytes = merged.worst_window_bytes.max(worst_window_with_prefix(
        &prefix,
        &parsed.lengths,
        config.window,
    ));
    merged.frames = merged
        .frames
        .checked_add(segment.frames as u64)
        .ok_or("merged frame count overflow")?;
    merged.segments += 1;
    let keep = config.window.saturating_sub(1);
    let mut tail = VecDeque::new();
    for &length in prefix.iter().chain(parsed.lengths.iter()) {
        tail.push_back(length);
        if tail.len() > keep {
            tail.pop_front();
        }
    }
    merged.tail = tail;

    fs::remove_file(&segment.y4m)
        .map_err(|error| format!("could not remove accepted segment Y4M: {error}"))?;
    fs::remove_file(&segment.tvc)
        .map_err(|error| format!("could not remove accepted segment TVC: {error}"))?;
    check_workspace_capacity(
        work,
        (workers.len() as u64)
            .checked_mul(MAX_ACCEPTED_SEGMENT_TVC_BYTES)
            .ok_or("workspace reserve overflow")?,
        config.options.workspace_limit_bytes,
    )?;
    Ok(())
}

fn append_tvc_chunks(bytes: &[u8], lengths: &[u32], output: &mut File) -> Result<(), String> {
    let mut offset = 20usize;
    for &expected in lengths {
        let actual = read_u32(bytes, offset)?;
        if actual != expected {
            return Err("TVC picture length changed while merging".into());
        }
        let end = offset
            .checked_add(4)
            .and_then(|at| at.checked_add(actual as usize))
            .ok_or("TVC merge offset overflow")?;
        if end > bytes.len() {
            return Err("truncated TVC segment while merging".into());
        }
        output
            .write_all(&bytes[offset..end])
            .map_err(|error| format!("could not append TVC chunk: {error}"))?;
        offset = end;
    }
    if offset != bytes.len() {
        return Err("trailing bytes in TVC segment during merge".into());
    }
    Ok(())
}

fn join_remaining(workers: &mut VecDeque<PendingSegment>) -> Result<(), String> {
    let mut first_error = None;
    while let Some(segment) = workers.pop_front() {
        let result = segment
            .worker
            .join()
            .map_err(|_| format!("segment {} worker panicked", segment.index))
            .and_then(|result| result);
        if first_error.is_none() {
            first_error = result.err();
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn rate_window(header: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(header).map_err(|_| "invalid Y4M header text")?;
    let mut fps = None;
    for token in text.split_ascii_whitespace() {
        if let Some(value) = token.strip_prefix('F') {
            if fps.is_some() {
                return Err("duplicate Y4M frame rate".into());
            }
            let (num, den) = value.split_once(':').ok_or("invalid Y4M frame rate")?;
            let num = num.parse::<usize>().map_err(|_| "invalid Y4M frame rate")?;
            let den = den.parse::<usize>().map_err(|_| "invalid Y4M frame rate")?;
            if num == 0 || den == 0 {
                return Err("invalid Y4M frame rate".into());
            }
            fps = Some(num.checked_add(den - 1).ok_or("Y4M frame rate overflow")? / den);
        }
    }
    fps.filter(|&window| window > 0)
        .ok_or_else(|| "missing/invalid Y4M frame rate".into())
}

fn worst_window_with_prefix(prefix: &[u32], next: &[u32], window: usize) -> u64 {
    let mut current = VecDeque::new();
    let mut worst = 0u64;
    for &length in prefix.iter().chain(next) {
        current.push_back(length);
        if current.len() > window {
            current.pop_front();
        }
        worst = worst.max(current.iter().map(|&value| u64::from(value)).sum());
    }
    worst
}

fn run_tvc(
    tool: &Path,
    input: &Path,
    output: &Path,
    qp: u8,
    level: u8,
    previous: Option<&str>,
) -> Result<(), String> {
    let mut command = Command::new(tool);
    command
        .arg(input)
        .arg(output)
        .args(["-q", &qp.to_string(), "-l", &level.to_string()]);
    if let Some(previous) = previous {
        command.args(["-P", previous]);
    }
    let status = command
        .status()
        .map_err(|error| format!("could not start tvc_enc: {error}"))?;
    if !status.success() {
        return Err(format!(
            "tvc_enc failed for {} with {status}",
            input.display()
        ));
    }
    Ok(())
}

fn run_status(tool: &Path, args: &[&str], label: &str) -> Result<(), String> {
    let status = Command::new(tool)
        .args(args)
        .status()
        .map_err(|error| format!("could not start {label}: {error}"))?;
    if !status.success() {
        return Err(format!("{label} failed with {status}"));
    }
    Ok(())
}

fn workspace_bytes(work: &Path) -> Result<u64, String> {
    let mut entries =
        fs::read_dir(work).map_err(|error| format!("could not inspect workspace: {error}"))?;
    entries
        .try_fold(0u64, |total, entry| {
            let entry = entry.map_err(|error| {
                io::Error::other(format!("could not inspect workspace entry: {error}"))
            })?;
            let metadata = entry.metadata().map_err(|error| {
                io::Error::other(format!("could not inspect workspace file: {error}"))
            })?;
            if !metadata.is_file() {
                return Err(io::Error::other(
                    "unexpected non-file in orchestration workspace",
                ));
            }
            total
                .checked_add(metadata.len())
                .ok_or_else(|| io::Error::other("workspace byte count overflow"))
        })
        .map_err(|error| error.to_string())
}

fn check_workspace_budget(work: &Path, limit: u64) -> Result<(), String> {
    check_workspace_capacity(work, 0, limit)
}

fn check_workspace_capacity(work: &Path, reserved: u64, limit: u64) -> Result<(), String> {
    let actual = workspace_bytes(work)?;
    let projected = actual
        .checked_add(reserved)
        .ok_or("workspace capacity overflow")?;
    if projected > limit {
        return Err(format!(
            "workspace limit exceeded: {actual} retained + {reserved} reserved > {limit} bytes"
        ));
    }
    Ok(())
}

struct ParsedTvc {
    header: Vec<u8>,
    lengths: Vec<u32>,
    bytes: Vec<u8>,
}
fn parse_tvc(path: &Path) -> Result<ParsedTvc, String> {
    let size = fs::metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
        .len();
    if size > MAX_ACCEPTED_SEGMENT_TVC_BYTES {
        return Err(format!(
            "TVC segment exceeds accepted {MAX_ACCEPTED_SEGMENT_TVC_BYTES} byte limit"
        ));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if bytes.len() < 20 || &bytes[..4] != b"TVC1" {
        return Err(format!("{} is not a TVC1 segment", path.display()));
    }
    let count = read_u32(&bytes, 12)? as usize;
    if count > MAX_SEGMENT_FRAMES {
        return Err("TVC frame count exceeds segment bound".into());
    }
    let mut offset = 20usize;
    let mut lengths = Vec::with_capacity(count);
    for _ in 0..count {
        let length = read_u32(&bytes, offset)?;
        offset = offset.checked_add(4).ok_or("TVC offset overflow")?;
        let end = offset
            .checked_add(length as usize)
            .ok_or("TVC chunk overflow")?;
        if end > bytes.len() {
            return Err(format!("truncated TVC chunk in {}", path.display()));
        }
        lengths.push(length);
        offset = end;
    }
    if offset != bytes.len() {
        return Err(format!(
            "unexpected trailing TVC bytes in {}",
            path.display()
        ));
    }
    Ok(ParsedTvc {
        header: bytes[..20].to_vec(),
        lengths,
        bytes,
    })
}
fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let end = at.checked_add(4).ok_or("TVC offset overflow")?;
    let chunk = bytes
        .get(at..end)
        .ok_or("truncated TVC header/chunk length")?;
    Ok(u32::from_le_bytes(
        chunk.try_into().map_err(|_| "invalid TVC u32")?,
    ))
}
fn check_level_fit(header: &[u8], level: u8) -> Result<(), String> {
    let text = std::str::from_utf8(header).map_err(|_| "invalid Y4M header text")?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    for token in text.split_ascii_whitespace() {
        if let Some(value) = token.strip_prefix('W') {
            width = Some(value.parse::<u64>().map_err(|_| "invalid Y4M width")?);
        } else if let Some(value) = token.strip_prefix('H') {
            height = Some(value.parse::<u64>().map_err(|_| "invalid Y4M height")?);
        } else if let Some(value) = token.strip_prefix('F') {
            if fps.is_some() {
                return Err("duplicate Y4M frame rate".into());
            }
            let (num, den) = value.split_once(':').ok_or("invalid Y4M frame rate")?;
            fps = Some((
                num.parse::<u64>().map_err(|_| "invalid Y4M frame rate")?,
                den.parse::<u64>().map_err(|_| "invalid Y4M frame rate")?,
            ));
        }
    }
    let width = width
        .filter(|&value| value > 0)
        .ok_or("missing Y4M width")?;
    let height = height
        .filter(|&value| value > 0)
        .ok_or("missing Y4M height")?;
    let (fps_num, fps_den) = fps
        .filter(|&(num, den)| num > 0 && den > 0)
        .ok_or("missing/invalid Y4M frame rate")?;
    if fps_num > u16::MAX as u64 || fps_den > u16::MAX as u64 {
        return Err("Y4M frame-rate numerator and denominator must fit the TVC u16 fields".into());
    }
    let (max_w, max_h, max_mbs, max_mbps): (u64, u64, u64, u64) = match level {
        1 => (400, 240, 375, 11_250),
        2 => (800, 464, 1_450, 43_500),
        3 => (800, 720, 2_250, 67_500),
        _ => return Err("level must be 1, 2, or 3".into()),
    };
    let padded_w = width.checked_add(15).ok_or("Y4M padded width overflow")? & !15;
    let padded_h = height.checked_add(15).ok_or("Y4M padded height overflow")? & !15;
    let mbs = (padded_w / 16)
        .checked_mul(padded_h / 16)
        .ok_or("Y4M macroblock count overflow")?;
    let mbps = mbs
        .checked_mul(fps_num)
        .ok_or("Y4M macroblocks per second overflow")?;
    let max_fps_numerator = 30u64
        .checked_mul(fps_den)
        .ok_or("Y4M frame rate overflow")?;
    let max_mbps_numerator = max_mbps
        .checked_mul(fps_den)
        .ok_or("Y4M macroblocks per second overflow")?;
    if width > max_w
        || height > max_h
        || mbs > max_mbs
        || fps_num > max_fps_numerator
        || mbps > max_mbps_numerator
    {
        return Err(format!(
            "{width}x{height} at {fps_num}/{fps_den} fps does not fit TVC level {level}"
        ));
    }
    Ok(())
}
fn window_ok(prefix: &[u32], next: &[u32], window: usize, limit: u64) -> bool {
    let mut current: VecDeque<u32> = VecDeque::new();
    for &length in prefix.iter().chain(next) {
        current.push_back(length);
        if current.len() > window {
            current.pop_front();
        }
        if current.iter().map(|&v| v as u64).sum::<u64>() > limit {
            return false;
        }
    }
    true
}
#[cfg(test)]
fn worst_window(lengths: &[u32], window: usize) -> u64 {
    let mut current: VecDeque<u32> = VecDeque::new();
    let mut worst = 0;
    for &length in lengths {
        current.push_back(length);
        if current.len() > window {
            current.pop_front();
        }
        worst = worst.max(current.iter().map(|&v| v as u64).sum());
    }
    worst
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn generated_y4m(frames: usize) -> Vec<u8> {
        let width = 64usize;
        let height = 48usize;
        let mut bytes = b"YUV4MPEG2 W64 H48 F24:1 Ip A1:1 C420jpeg\n".to_vec();
        let y_size = width * height;
        let c_size = width.div_ceil(2) * height.div_ceil(2);
        for frame in 0..frames {
            bytes.extend_from_slice(format!("FRAME Xtimestamp={frame}\n").as_bytes());
            bytes.extend((0..y_size).map(|i| ((i * 3 + frame * 5 + i / width * 11) & 255) as u8));
            bytes.extend((0..(2 * c_size)).map(|i| ((i * 7 + frame * 3) & 255) as u8));
        }
        bytes
    }

    #[test]
    fn window_helpers_detect_cross_segment_overflow() {
        assert!(window_ok(&[40, 40], &[40], 3, 120));
        assert!(!window_ok(&[50, 50], &[30], 3, 120));
        assert_eq!(worst_window(&[20, 50, 30, 40], 2), 80);
    }

    #[test]
    fn segment_limit_allows_eof_at_exact_cap_and_rejects_next_group() {
        assert!(validate_next_segment(0, 1).is_ok());
        assert!(validate_next_segment(1, 1).is_err());
        assert!(validate_next_segment(MAX_SEGMENTS - 1, MAX_SEGMENTS).is_ok());
        assert!(validate_next_segment(MAX_SEGMENTS, MAX_SEGMENTS).is_err());
    }

    #[test]
    fn options_are_bounded() {
        assert!(
            validate_options(Options {
                frames_per_segment: 0,
                ..Options::default()
            })
            .is_err()
        );
        assert!(
            validate_options(Options {
                jobs: MAX_JOBS + 1,
                ..Options::default()
            })
            .is_err()
        );
        assert!(
            validate_options(Options {
                level: 4,
                ..Options::default()
            })
            .is_err()
        );
        assert!(
            validate_options(Options {
                workspace_limit_bytes: 0,
                ..Options::default()
            })
            .is_err()
        );
    }

    #[test]
    fn desktop_480_frame_segment_fits_y4m_and_projected_workspace_caps() {
        let y4m_header = b"YUV4MPEG2 W400 H360 F24:1 C420jpeg\n";
        let frame_bytes = 400u64 * 360 + 2 * 200 * 180;
        let segment_bytes = frame_bytes * 480 + y4m_header.len() as u64 + 480 * 6;
        assert!(segment_bytes <= MAX_SEGMENT_Y4M_BYTES);
        assert!(segment_bytes + 6 * MAX_ACCEPTED_SEGMENT_TVC_BYTES < DEFAULT_WORKSPACE_LIMIT_BYTES);
    }

    #[test]
    fn wrapped_tvc_frame_rate_fields_are_rejected_before_encoder_start() {
        assert!(check_level_fit(b"YUV4MPEG2 W64 H48 F24:1 C420jpeg\n", 1).is_ok());
        assert!(
            check_level_fit(b"YUV4MPEG2 W64 H48 F65536:1 C420jpeg\n", 1)
                .unwrap_err()
                .contains("u16")
        );
        assert!(
            check_level_fit(b"YUV4MPEG2 W64 H48 F24:65536 C420jpeg\n", 1)
                .unwrap_err()
                .contains("u16")
        );
        assert!(check_level_fit(b"YUV4MPEG2 W64 H48 F24:1 F24:1 C420jpeg\n", 1).is_err());
    }

    #[test]
    fn segment_byte_limit_counts_header_and_metadata() {
        let bytes = generated_y4m(1);
        let mut groups = Y4mFrameGroups::new(BufReader::new(Cursor::new(bytes)), 1).unwrap();
        let header_len = groups.header().len() as u64;
        let metadata_len = b"FRAME Xtimestamp=0\n".len() as u64;
        let limit = header_len + metadata_len + groups.frame_bytes() - 1;
        let mut writer = BoundedWriter::new(Vec::new(), limit);
        assert_eq!(
            groups.write_next_group(&mut writer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(writer.written, header_len + metadata_len);
        assert!(writer.written <= limit);
    }

    #[test]
    fn generated_fixture_matches_python_reference_when_tools_are_configured() {
        let Some(python) = std::env::var_os("TAU_TEST_PYTHON") else {
            return;
        };
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else {
            return;
        };
        let Some(ffprobe) = std::env::var_os("TAU_TEST_FFPROBE") else {
            return;
        };
        let Some(tvc) = std::env::var_os("TAU_TEST_TVC") else {
            return;
        };
        let Some(tnv3) = std::env::var_os("TAU_TEST_TNV3") else {
            return;
        };
        let Some(c_script) = std::env::var_os("TAU_TEST_C_SCRIPT") else {
            return;
        };
        let source = generated_y4m(9);
        let temp = TempDir::new().unwrap();
        let source_path = temp.path.join("fixture.y4m");
        fs::write(&source_path, &source).unwrap();
        let tools = Tools {
            tvc_encoder: PathBuf::from(&tvc),
            tnv3: PathBuf::from(&tnv3),
        };
        let mut rust_output = Vec::new();
        let report = encode_y4m_video(
            &tools,
            BufReader::new(Cursor::new(source.clone())),
            &mut rust_output,
            Options {
                qp: 30,
                level: 1,
                frames_per_segment: 3,
                jobs: 2,
                workspace_limit_bytes: DEFAULT_WORKSPACE_LIMIT_BYTES,
            },
        )
        .unwrap();
        assert_eq!((report.frames, report.segments), (9, 3));
        assert_eq!(report.joint_reencodes, 0);
        assert!(report.worst_window_bytes <= LEVEL_WINDOW_BYTES[1]);
        let python_output = temp.path.join("python.tnv3");
        let work = temp.path.join("python-work");
        fs::create_dir(&work).unwrap();
        let result = Command::new(python)
            .arg(c_script)
            .arg(&source_path)
            .arg(&python_output)
            .args([
                "--video-only",
                "--width",
                "native",
                "--fps",
                "24",
                "--seg",
                "3",
                "--jobs",
                "2",
                "--qp",
                "30",
                "--work",
            ])
            .arg(&work)
            .env("TAU_TVC_ENC", &tvc)
            .env("TAU_TNV3", &tnv3)
            .env("TAU_FFMPEG", &ffmpeg)
            .env("TAU_FFPROBE", &ffprobe)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "python reference failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(rust_output, fs::read(&python_output).unwrap());
        temp.cleanup().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stub_encoder_forces_boundary_retry_and_rate_recovery() {
        if Command::new("cc").arg("--version").output().is_err() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().unwrap();
        let fake_tvc_path = temp.path.join("fake-tvc.c");
        let fake_tvc = temp.path.join("fake-tvc");
        let retry_log = temp.path.join("retry.log");
        let log_c = retry_log
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let c_source = format!(
            r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void put16(FILE *f, unsigned v) {{ fputc(v & 255, f); fputc((v >> 8) & 255, f); }}
static void put32(FILE *f, unsigned v) {{ for (int i=0;i<4;i++) fputc((v >> (8*i)) & 255, f); }}
int main(int argc, char **argv) {{
  if (argc < 3) return 2;
  int retry=0, level=1; const char *previous="";
  for (int i=3; i+1<argc; i+=2) {{
    if (!strcmp(argv[i], "-P")) {{ retry=1; previous=argv[i+1]; }}
    if (!strcmp(argv[i], "-l")) level=atoi(argv[i+1]);
  }}
  FILE *in=fopen(argv[1], "rb"); if (!in) return 3;
  char line[512]; if (!fgets(line, sizeof line, in)) return 4;
  int w=0,h=0,fn=24,fd=1;
  for (char *t=strtok(line, " \n"); t; t=strtok(NULL, " \n")) {{
    if (t[0]=='W') w=atoi(t+1); else if (t[0]=='H') h=atoi(t+1);
    else if (t[0]=='F') sscanf(t+1, "%d:%d", &fn, &fd);
  }}
  size_t frame_size=(size_t)w*h + 2*(size_t)((w+1)/2)*((h+1)/2);
  unsigned char *frame=malloc(frame_size); if (!frame) return 5;
  int count=0;
  while (fgets(line, sizeof line, in)) {{
    if (strncmp(line,"FRAME",5) || fread(frame,1,frame_size,in)!=frame_size) return 6;
    count++;
  }}
  fclose(in);
  FILE *out=fopen(argv[2], "wb"); if (!out) return 7;
  fwrite("TVC1",1,4,out); put16(out,(unsigned)w); put16(out,(unsigned)h);
  put16(out,(unsigned)fn); put16(out,(unsigned)fd); put32(out,(unsigned)count);
  fputc(level,out); fputc(0,out); fputc(0,out); fputc(0,out);
  unsigned len=retry ? 6000 : 10000; unsigned char *pic=calloc(len,1); if (!pic) return 8;
  for (int i=0;i<count;i++) {{ put32(out,len); fwrite(pic,1,len,out); }}
  fclose(out); free(pic); free(frame);
  if (retry) {{ FILE *log=fopen("{log_c}","a"); if (!log) return 9; fprintf(log,"%s\n",previous); fclose(log); }}
  return 0;
}}
"#
        );
        fs::write(&fake_tvc_path, c_source).unwrap();
        let compile = Command::new("cc")
            .args(["-O2"])
            .arg(&fake_tvc_path)
            .arg("-o")
            .arg(&fake_tvc)
            .status()
            .unwrap();
        assert!(compile.success());

        let fake_tnv3 = temp.path.join("fake-tnv3");
        fs::write(&fake_tnv3, "#!/bin/sh\ncase \"$1\" in\n  mux) merge=\"$2\"; dir=$(dirname \"$merge\"); for f in \"$dir\"/segment*.y4m \"$dir\"/segment*.tvc; do [ ! -e \"$f\" ] || exit 10; done; shift; while [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then printf mock-valid > \"$2\"; exit 0; fi; shift; done; exit 2;;\n  check) [ \"$(cat \"$2\")\" = mock-valid ];;\n  *) exit 2;;\nesac\n").unwrap();
        let mut perms = fs::metadata(&fake_tnv3).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&fake_tnv3, perms).unwrap();

        let tools = Tools {
            tvc_encoder: fake_tvc,
            tnv3: fake_tnv3,
        };
        let mut output = Vec::new();
        let report = encode_y4m_video(
            &tools,
            BufReader::new(Cursor::new(generated_y4m(36))),
            &mut output,
            Options {
                qp: 30,
                level: 1,
                frames_per_segment: 12,
                jobs: 2,
                workspace_limit_bytes: DEFAULT_WORKSPACE_LIMIT_BYTES,
            },
        )
        .unwrap();
        assert_eq!(
            (report.frames, report.segments, report.joint_reencodes),
            (36, 3, 1)
        );
        assert_eq!(report.worst_window_bytes, 192_000);
        assert_eq!(output, b"mock-valid");
        let retry = fs::read_to_string(&retry_log).unwrap();
        assert_eq!(
            retry.trim(),
            std::iter::repeat_n("10000", 12)
                .collect::<Vec<_>>()
                .join(",")
        );
        temp.cleanup().unwrap();
    }
}

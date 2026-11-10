//! C-reference workflow from an already decoded Y4M stream (the default encoder workflow of the app and the CLI).
//!
//! This vertical slice groups Y4M frames, runs bounded concurrent `tvc_enc` segment jobs,
//! repairs cross-segment rate windows with `-P`, merges TVC1 chunks, then muxes and checks a
//! video-only TNV3. It does not replace the Python workflow or invoke FFmpeg itself.
use crate::y4m::Y4mFrameGroups;
use crate::source_gate::{GatedReader, SourceGate};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ops::{Deref, DerefMut},
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc},
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

/// Executables used by this workflow.
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
struct MonitorFinish<'a>(&'a AtomicBool);
impl Drop for MonitorFinish<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
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
    fn cleanup(mut self) -> Result<(), (PathBuf, io::Error)> {
        self.armed = false;
        if let Err(error) = fs::remove_dir_all(&self.path) {
            return Err((self.path.clone(), error));
        }
        Ok(())
    }
    fn preserve(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
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
    encode_y4m_video_with_progress(tools, input, output, options, |_| {})
}

/// As `encode_y4m_video`, reporting frames only after each segment is accepted.
pub fn encode_y4m_video_with_progress<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    progress: impl FnMut(u64),
) -> Result<Report, String> {
    encode_y4m_video_with_progress_and_quiet_children(
        tools, input, output, options, false, progress,
    )
}

/// As `encode_y4m_video_with_progress`, optionally suppressing child stdout for
/// callers that reserve stdout for a machine-readable protocol.
pub fn encode_y4m_video_with_progress_and_quiet_children<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    quiet_child_stdout: bool,
    progress: impl FnMut(u64),
) -> Result<Report, String> {
    encode_y4m_video_with_control_and_quiet_children(
        tools, input, output, options, None, quiet_child_stdout, progress,
    )
}

/// Cooperative cancellation. On Stop, termination of a running
/// direct C/mux child is attempted and reaping is polled for a bounded time.
/// Unverified cleanup retains the workspace; eventual reaping is not guaranteed.
/// Segment workers are joined, so unrelated blocked I/O can still delay return.
/// A blocked input read requires its source owner to stop/reap the producer.
/// Descendants are not contained; tools must be trusted leaf executables.
/// Callers must keep `output` staged and serialize cancellation with publication.
pub fn encode_y4m_video_with_control<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    stop: Option<&AtomicBool>,
    progress: impl FnMut(u64),
) -> Result<Report, String> {
    encode_y4m_video_with_control_and_quiet_children(
        tools, input, output, options, stop, false, progress,
    )
}

/// Combine cooperative stop with optional child-stdout suppression.
/// The caller must supervise the producer and keep output staged until its
/// exit status and cancellation state are verified.
pub fn encode_y4m_video_with_control_and_quiet_children<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    stop: Option<&AtomicBool>,
    quiet_child_stdout: bool,
    mut progress: impl FnMut(u64),
) -> Result<Report, String> {
    validate_options(options)?;
    check_stop(stop)?;
    let temp = TempDir::new()
        .map_err(|error| format!("could not create private work directory: {error}"))?;
    let cleanup_verified = Arc::new(AtomicBool::new(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        encode_in_workspace(
            tools, input, output, options, &temp.path, stop, quiet_child_stdout, &mut progress, &cleanup_verified,
        )
    }))
    .unwrap_or_else(|panic| Err(format!("orchestrator panicked: {}", panic_message(panic))));
    if !cleanup_verified.load(Ordering::Acquire) {
        let path = temp.preserve();
        return Err(match result {
            Ok(_) => format!("encode completed but child cleanup is unverified; work directory retained at {}", path.display()),
            Err(error) => format!("{error}; child cleanup is unverified, work directory retained at {}", path.display()),
        });
    }
    let cleanup = temp.cleanup();
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err((path, cleanup))) => Err(format!(
            "encode succeeded but work directory cleanup failed: {cleanup}; remaining path: {}", path.display()
        )),
        (Err(error), Err((path, cleanup))) => Err(format!(
            "{error}; work directory cleanup failed: {cleanup}; remaining path: {}", path.display()
        )),
    }
}

/// Live-decision entry point. The caller must run a separate
/// diagnostic drainer and supervise the source decoder process.
pub fn encode_y4m_video_with_gate<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    stop: Option<&AtomicBool>,
    gate: Arc<SourceGate>,
    progress: impl FnMut(u64),
) -> Result<Report, String> {
    encode_y4m_video_with_control(
        tools, GatedReader { inner: input, gate }, output, options, stop, progress,
    )
}

fn check_stop(stop: Option<&AtomicBool>) -> Result<(), String> {
    if stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        Err("encoding stopped before output publication".into())
    } else {
        Ok(())
    }
}

fn call_progress(progress: &mut impl FnMut(u64), frames: u64) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| progress(frames)))
        .map_err(|_| "progress callback panicked; workers were cancelled and joined".to_string())
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = panic.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else {
        "non-string panic payload".into()
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

struct PendingWorkers {
    items: VecDeque<PendingSegment>,
    cancel: Arc<AtomicBool>,
    cleanup_verified: Arc<AtomicBool>,
}
impl PendingWorkers {
    fn new(cancel: Arc<AtomicBool>, cleanup_verified: Arc<AtomicBool>) -> Self {
        Self { items: VecDeque::new(), cancel, cleanup_verified }
    }
}
impl Deref for PendingWorkers {
    type Target = VecDeque<PendingSegment>;
    fn deref(&self) -> &Self::Target { &self.items }
}
impl DerefMut for PendingWorkers {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.items }
}
impl Drop for PendingWorkers {
    fn drop(&mut self) {
        if !self.items.is_empty() {
            self.cancel.store(true, Ordering::Release);
        }
        while let Some(segment) = self.items.pop_front() {
            if segment.worker.join().is_err() {
                self.cleanup_verified.store(false, Ordering::Release);
            }
        }
    }
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
    quiet_child_stdout: bool,
}

fn encode_in_workspace<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    work: &Path,
    stop: Option<&AtomicBool>,
    quiet_child_stdout: bool,
    progress: &mut impl FnMut(u64),
    cleanup_verified: &Arc<AtomicBool>,
) -> Result<Report, String> {
    let worker_stop = Arc::new(AtomicBool::new(false));
    let monitor_finished = AtomicBool::new(false);
    thread::scope(|scope| {
        let _finish = MonitorFinish(&monitor_finished);
        if let Some(caller_stop) = stop {
            let stop_token = Arc::clone(&worker_stop);
            let finished = &monitor_finished;
            scope.spawn(move || {
                while !finished.load(Ordering::Acquire) {
                    if caller_stop.load(Ordering::Acquire) {
                        stop_token.store(true, Ordering::Release);
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(10));
                }
            });
        }
        let result = encode_in_workspace_inner(
            tools, input, output, options, work, stop, quiet_child_stdout, progress, &worker_stop, cleanup_verified,
        );
        result
    })
}

fn encode_in_workspace_inner<R: BufRead, W: Write>(
    tools: &Tools,
    input: R,
    output: &mut W,
    options: Options,
    work: &Path,
    stop: Option<&AtomicBool>,
    quiet_child_stdout: bool,
    progress: &mut impl FnMut(u64),
    worker_stop: &Arc<AtomicBool>,
    cleanup_verified: &Arc<AtomicBool>,
) -> Result<Report, String> {
    check_stop(stop)?;
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
        quiet_child_stdout,
    };
    // Workers own their C children. A failed split/merge path flips this shared
    // token before joining, so every still-running encoder can be killed and reaped.
    let mut workers = PendingWorkers::new(Arc::clone(worker_stop), Arc::clone(cleanup_verified));
    let mut merged = MergeState::default();
    let mut next_segment = 0u32;
    let split_result = (|| -> Result<(), String> {
        loop {
            check_stop(stop)?;
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
            check_stop(stop)?;
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
            let quiet_child_stdout = quiet_child_stdout;
            let worker_input = y4m.clone();
            let worker_output = tvc.clone();
            let worker_stop_token = Arc::clone(worker_stop);
            let worker_cleanup_verified = Arc::clone(cleanup_verified);
            let worker = thread::Builder::new()
                .name(format!("tvc-segment-{next_segment}"))
                .spawn(move || {
                    run_tvc(
                        &tvc_tool,
                        &worker_input,
                        &worker_output,
                        qp,
                        level,
                        None,
                        &worker_stop_token,
                        None,
                        &worker_cleanup_verified,
                        quiet_child_stdout,
                    )
                })
                .map_err(|error| format!("could not start segment worker: {error}"))?;
            workers.push_back(PendingSegment {
                index: next_segment,
                frames,
                y4m,
                tvc,
                worker,
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
                accept_next_segment(tools, work, &mut workers, &mut merged, config, stop, worker_stop.as_ref(), cleanup_verified)?;
                call_progress(progress, merged.frames)?;
            }
        }
        while !workers.is_empty() {
            check_stop(stop)?;
            accept_next_segment(tools, work, &mut workers, &mut merged, config, stop, worker_stop.as_ref(), cleanup_verified)?;
            call_progress(progress, merged.frames)?;
        }
        Ok(())
    })();
    if split_result.is_err() {
        worker_stop.store(true, Ordering::Release);
    }
    let worker_result = join_remaining(&mut workers, cleanup_verified);
    match (split_result, worker_result) {
        (Ok(()), Ok(())) => {}
        (Err(split), Ok(())) => return Err(split),
        (Ok(()), Err(workers)) => return Err(workers),
        (Err(split), Err(workers)) => {
            return Err(format!("{split}; outstanding worker result(s): {workers}"));
        }
    }
    check_stop(stop)?;
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
    check_stop(stop)?;
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
        stop,
        cleanup_verified,
        quiet_child_stdout,
    )?;
    check_workspace_budget(work, options.workspace_limit_bytes)?;
    check_stop(stop)?;
    run_status(
        &tools.tnv3,
        &["check", output_path.to_str().ok_or("non-UTF8 work path")?],
        "tnv3 check",
        stop,
        cleanup_verified,
        quiet_child_stdout,
    )?;
    let metadata = fs::metadata(&output_path)
        .map_err(|error| format!("could not inspect checked TNV3: {error}"))?;
    let mut encoded = File::open(&output_path)
        .map_err(|error| format!("could not open checked TNV3: {error}"))?;
    check_stop(stop)?;
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
    external_stop: Option<&AtomicBool>,
    worker_stop: &AtomicBool,
    cleanup_verified: &AtomicBool,
) -> Result<(), String> {
    let segment = workers.pop_front().ok_or("missing segment worker")?;
    let joined = match join_worker_stop_aware(segment.worker, external_stop, worker_stop) {
        Ok(joined) => joined,
        Err(_) => {
            cleanup_verified.store(false, Ordering::Release);
            return Err(format!("segment {} worker panicked", segment.index));
        }
    };
    joined?;
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
            worker_stop,
            external_stop,
            cleanup_verified,
            config.quiet_child_stdout,
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

fn join_remaining(workers: &mut VecDeque<PendingSegment>, cleanup_verified: &AtomicBool) -> Result<(), String> {
    let mut errors = Vec::new();
    while let Some(segment) = workers.pop_front() {
        let result = segment
            .worker
            .join()
            .map_err(|_| {
                cleanup_verified.store(false, Ordering::Release);
                format!("segment {} worker panicked", segment.index)
            })
            .and_then(|result| result);
        if let Err(error) = result {
            errors.push(format!("segment {}: {error}", segment.index));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn join_worker_stop_aware(
    worker: JoinHandle<Result<(), String>>,
    external_stop: Option<&AtomicBool>,
    worker_stop: &AtomicBool,
) -> Result<Result<(), String>, Box<dyn std::any::Any + Send + 'static>> {
    while !worker.is_finished() {
        if external_stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            worker_stop.store(true, Ordering::Release);
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    worker.join()
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
    stop: &AtomicBool,
    external_stop: Option<&AtomicBool>,
    cleanup_verified: &AtomicBool,
    quiet_child_stdout: bool,
) -> Result<(), String> {
    if stop.load(Ordering::Acquire) || external_stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err("encoding stopped before starting tvc_enc".into());
    }
    let mut command = Command::new(tool);
    command
        .arg(input)
        .arg(output)
        .args(["-q", &qp.to_string(), "-l", &level.to_string()])
        .stdout(if quiet_child_stdout { Stdio::null() } else { Stdio::inherit() });
    if let Some(previous) = previous {
        command.args(["-P", previous]);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start tvc_enc: {error}"))?;
    loop {
        if stop.load(Ordering::Acquire) || external_stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return terminate_and_reap(child, &input, "encoding stopped", cleanup_verified);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("tvc_enc failed for {} with {status}", input.display()));
            }
            Ok(None) => thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => {
                // An uninspectable identity must not be signalled again.
                return Err(retain_child_for_reaping(child, &format!("tvc_enc for {}", input.display()),
                    &format!("could not inspect child: {error}"), cleanup_verified));
            }
        }
    }
}

/// The worker owns its child through normal completion; failed wait attempts
/// transfer ownership to a dedicated retrying reaper and keep cleanup unverified.
fn terminate_and_reap(
    child: std::process::Child,
    input: &Path,
    reason: &str,
    cleanup_verified: &AtomicBool,
) -> Result<(), String> {
    terminate_child_and_reap(child, &format!("tvc_enc for {}", input.display()), reason, cleanup_verified)
}

fn terminate_child_and_reap(
    mut child: std::process::Child,
    label: &str,
    reason: &str,
    cleanup_verified: &AtomicBool,
) -> Result<(), String> {
    let kill_error = child.kill().err();
    match bounded_child_wait(&mut child, CHILD_REAP_TIMEOUT) {
        Ok(_status) if kill_error.is_none() => Err(format!("{reason}: {label} was killed and reaped")),
        Ok(status) if !status.success() => Err(format!("{reason}: kill failed for {label} ({:?}); child exited ({status})", kill_error)),
        Ok(status) => Err(format!("{reason}: kill failed for {label} ({:?}); child exited with {status}", kill_error)),
        Err(wait_error) => {
            let detail = match kill_error {
                Some(kill_error) => format!("kill failed ({kill_error}); wait failed ({wait_error})"),
                None => format!("wait failed ({wait_error})"),
            };
            Err(retain_child_for_reaping(child, label, &format!("{reason}: {detail}"), cleanup_verified))
        }
    }
}

const CHILD_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn bounded_child_wait(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> io::Result<std::process::ExitStatus> {
    bounded_child_wait_with(|| child.try_wait(), timeout)
}

fn bounded_child_wait_with(
    mut poll: impl FnMut() -> io::Result<Option<std::process::ExitStatus>>,
    timeout: std::time::Duration,
) -> io::Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match poll() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child did not exit during bounded wait",
            )),
            Err(error) if error.kind() == io::ErrorKind::Interrupted
                && std::time::Instant::now() < deadline =>
            {
                thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => return Err(io::Error::new(
                error.kind(),
                format!("child wait failed; stopping retries: {error}"),
            )),
        }
    }
}

/// Transfers an unverified direct child to a bounded private reaper. The
/// reaper may drop a still-live Child after timeout or permanent wait failure;
/// eventual reaping is not guaranteed. If its thread cannot be created, this
/// function returns immediately and cleanup remains unverified.
fn retain_child_for_reaping(
    child: std::process::Child,
    label: &str,
    reason: &str,
    cleanup_verified: &AtomicBool,
) -> String {
    use std::sync::Mutex;
    cleanup_verified.store(false, Ordering::Release);
    let owner = Arc::new(Mutex::new(Some(child)));
    let background_owner = Arc::clone(&owner);
    match thread::Builder::new()
        .name("tau-native-child-reaper".into())
        .spawn(move || {
            let child = match background_owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            if let Some(mut child) = child {
                let _ = bounded_child_wait(&mut child, CHILD_REAP_TIMEOUT);
            }
        })
    {
        Ok(_owner) => format!("{reason}; cleanup is unverified; bounded background reaping was requested for {label}"),
        Err(spawn_error) => {
            let child = match owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            failed_reaper_spawn(child, label, reason, &spawn_error)
        }
    }
}

fn failed_reaper_spawn<T>(
    owner: Option<T>,
    label: &str,
    reason: &str,
    spawn_error: &io::Error,
) -> String {
    drop(owner);
    format!("{reason}; cleanup is unverified; background owner creation failed ({spawn_error}); no synchronous wait was attempted for {label}")
}

fn run_status(
    tool: &Path,
    args: &[&str],
    label: &str,
    stop: Option<&AtomicBool>,
    cleanup_verified: &AtomicBool,
    quiet_child_stdout: bool,
) -> Result<(), String> {
    if stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(format!("encoding stopped before starting {label}"));
    }
    let mut child = Command::new(tool)
        .args(args)
        .stdout(if quiet_child_stdout { Stdio::null() } else { Stdio::inherit() })
        .spawn()
        .map_err(|error| format!("could not start {label}: {error}"))?;
    loop {
        if stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return terminate_child_and_reap(child, label, "encoding stopped", cleanup_verified);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{label} failed with {status}")),
            Ok(None) => thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => {
                return Err(retain_child_for_reaping(child, label,
                    &format!("could not inspect child: {error}"), cleanup_verified));
            }
        }
    }
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
/// Check a Y4M header against the dimensions and decoding-work limits of a TVC level.
pub fn check_level_fit(header: &[u8], level: u8) -> Result<(), String> {
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
    #[test]
    fn bounded_child_wait_stops_on_permanent_wait_error() {
        use super::*;
        let calls = std::cell::Cell::new(0);
        let started = std::time::Instant::now();
        let error = bounded_child_wait_with(|| {
            calls.set(calls.get() + 1);
            Err(io::Error::other("injected ECHILD"))
        }, std::time::Duration::from_secs(2)).unwrap_err();
        assert_eq!(calls.get(), 1);
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
        assert!(error.to_string().contains("stopping retries"));
    }

    #[test]
    fn bounded_child_wait_retries_interrupted_only_until_deadline() {
        use super::*;
        let calls = std::cell::Cell::new(0);
        let started = std::time::Instant::now();
        let error = bounded_child_wait_with(|| {
            calls.set(calls.get() + 1);
            Err(io::Error::new(io::ErrorKind::Interrupted, "injected EINTR"))
        }, std::time::Duration::from_millis(60)).unwrap_err();
        assert!(calls.get() >= 2);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn bounded_child_wait_times_out_without_a_process_fixture() {
        use super::*;
        let started = std::time::Instant::now();
        let error = bounded_child_wait_with(|| Ok(None), std::time::Duration::from_millis(60)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn failed_reaper_spawn_drops_fake_owner_and_marks_unverified() {
        use super::*;
        struct DropMarker(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropMarker {
            fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let message = failed_reaper_spawn(
            Some(DropMarker(dropped.clone())),
            "fixture",
            "test cleanup",
            &io::Error::other("injected thread spawn failure"),
        );
        assert!(dropped.load(Ordering::SeqCst));
        assert!(message.contains("cleanup is unverified"));
        assert!(message.contains("no synchronous wait was attempted"));
    }

    #[test]
    fn staged_stop_before_input_leaves_writer_untouched() {
        use super::*;
        let stop = AtomicBool::new(true);
        let tools = Tools { tvc_encoder: "unused".into(), tnv3: "unused".into() };
        let mut out = Vec::new();
        let result = encode_y4m_video_with_control(
            &tools, std::io::Cursor::new(Vec::<u8>::new()), &mut out,
            Options::default(), Some(&stop), |_| {},
        );
        assert!(result.unwrap_err().contains("stopped"));
        assert!(out.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn stop_kills_and_joins_c_worker_without_publishing_output() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let root = std::env::temp_dir().join(format!(
            "tau-cancel-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let encoder = root.join("fake-tvc-encoder");
        fs::write(
            &encoder,
            b"#!/bin/sh\ncase \"$0\" in */*) marker_dir=${0%/*};; *) marker_dir=.;; esac\nprintf started > \"$marker_dir/started\"\nwhile :; do :; done\n",
        ).unwrap();
        let mut permissions = fs::metadata(&encoder).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&encoder, permissions).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let watch_root = root.clone();
        let watcher = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let worker_started = fs::read(&watch_root.join("started"))
                    .is_ok_and(|bytes| bytes == b"started");
                if worker_started {
                    signal.store(true, Ordering::Release);
                    return;
                }
                assert!(Instant::now() < deadline, "fake encoder did not start");
                thread::sleep(Duration::from_millis(5));
            }
        });

        let tools = Tools { tvc_encoder: encoder, tnv3: root.join("unused-tnv3") };
        let mut output = Vec::new();
        let result = encode_y4m_video_with_control(
            &tools,
            Cursor::new(generated_y4m(1)),
            &mut output,
            Options { frames_per_segment: 1, ..Options::default() },
            Some(&stop),
            |_| {},
        );
        watcher.join().unwrap();
        assert!(result.unwrap_err().contains("stopped"));
        assert!(output.is_empty(), "cancelled staged encode published bytes");
        fs::remove_dir_all(root).unwrap();
    }
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

//! Safe process-based adapter for the existing Python/C TNV3 film workflow.
//!
//! This invokes external programs; applications must distribute or configure
//! matching Python, FFmpeg, FFprobe, script and C executables separately.
//! It does not use FFI, encode audio or provide a streaming codec implementation.
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Paths to the existing reference tools. Bare program names may be resolved via PATH.
#[derive(Clone, Debug)]
pub struct Tools {
    /// Python 3 interpreter.
    pub python: PathBuf,
    /// Reference `encode_film.py` script.
    pub script: PathBuf,
    /// FFmpeg executable.
    pub ffmpeg: PathBuf,
    /// FFprobe executable.
    pub ffprobe: PathBuf,
    /// Override the script's default TVC encoder location.
    pub tvc_encoder: Option<PathBuf>,
    /// Override the script's default TNV3 tool location.
    pub tnv3_tool: Option<PathBuf>,
}
/// Optional experimental timestamp rounding during ingest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameTiming {
    /// Existing FFmpeg rounding and end-of-stream behavior.
    #[default]
    Default,
    /// Experimental rounding down, which may omit the final frame.
    RoundDown,
    /// Rounding down with final-frame pass, which can add a frame.
    RoundDownEofPass,
}
/// Encoding configuration for the desktop-compatible video-only TNV3 workflow.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Initial TVC quantizer, 0 through 51.
    pub qp: u8,
    /// Maximum parallel segment jobs and encoder CPU slots, at least one.
    pub jobs: usize,
    /// Source frame-rate conversion policy.
    pub timing: FrameTiming,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            qp: 30,
            jobs: 1,
            timing: FrameTiming::Default,
        }
    }
}
/// A bounded summary of the successful encoding operation.
#[derive(Debug)]
pub struct Report {
    /// Number of output pictures.
    pub frames: u32,
    /// Chosen TVC level.
    pub level: u8,
    /// Pictures whose quantizer was raised for level limits.
    pub reencoded: u32,
    /// Up to 4096 bytes of process diagnostics; may include source decoder warnings.
    pub diagnostics: String,
}
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn temp_dir() -> io::Result<TempDir> {
    for _ in 0..100 {
        let p = std::env::temp_dir().join(format!(
            "tau-c-encoder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match create_private_dir(&p) {
            Ok(()) => return Ok(TempDir(p)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not reserve encoder work directory",
    ))
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    // Windows keeps the parent directory's inherited ACL; no explicit ACL
    // hardening is performed here yet.
    fs::create_dir(path)
}
fn capture(mut input: impl Read) -> io::Result<String> {
    let mut saved = Vec::new();
    let mut buf = [0; 4096];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let take = n.min(4096usize.saturating_sub(saved.len()));
        saved.extend_from_slice(&buf[..take]);
    }
    Ok(String::from_utf8_lossy(&saved).trim().to_owned())
}
/// Encode a source into a video-only TNV3 stream using the C reference workflow.
///
/// The writer is touched only after the external encode and report succeed, but
/// an I/O failure while copying can leave partial output; callers own cleanup.
/// The synchronous callback receives completed/total segment counts. It must
/// return promptly. Audio sidecars remain the caller's responsibility.
pub fn encode_video<W: Write>(
    tools: &Tools,
    input: &Path,
    output: &mut W,
    options: Options,
    progress: impl FnMut(u32, u32),
) -> Result<Report, String> {
    encode_video_with_geometry(tools, input, output, options, 400, 360, 0, progress)
}

/// Encode with a maximum output size and an optional explicit TVC level (0 means automatic).
#[allow(clippy::too_many_arguments)]
pub fn encode_video_with_geometry<W: Write>(
    tools: &Tools,
    input: &Path,
    output: &mut W,
    options: Options,
    width: u32,
    height: u32,
    level: u8,
    mut progress: impl FnMut(u32, u32),
) -> Result<Report, String> {
    if options.qp > 51 || options.jobs == 0 {
        return Err("QP must be 0–51 and jobs must be positive".into());
    }
    if !(2..=800).contains(&width)
        || !(2..=720).contains(&height)
        || width % 2 != 0
        || height % 2 != 0
        || level > 3
    {
        return Err("Output dimensions must be even and at most 800x720; level must be 0–3".into());
    }
    let tmp = temp_dir().map_err(|e| format!("Cannot create encoder workspace: {e}"))?;
    let video = tmp.0.join("output.tnv3");
    let work = tmp.0.join("segments");
    let mut command = Command::new(&tools.python);
    command
        .arg(&tools.script)
        .arg(input)
        .arg(&video)
        .args(["--video-only", "--match-app", "--qp"])
        .arg(options.qp.to_string())
        .args(["--jobs"])
        .arg(options.jobs.to_string())
        .args(["--cpu-budget"])
        .arg(options.jobs.to_string())
        .arg("--app-size")
        .arg(format!("{width}x{height}"))
        .arg("--level")
        .arg(level.to_string())
        .arg("--seg")
        .arg(
            (480u32 * 400 * 360 / (width * height).max(1))
                .clamp(1, 480)
                .to_string(),
        )
        .args(["--work"])
        .arg(work)
        .env("TAU_FFMPEG", &tools.ffmpeg)
        .env("TAU_FFPROBE", &tools.ffprobe)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = &tools.tvc_encoder {
        command.env("TAU_TVC_ENC", path);
    }
    if let Some(path) = &tools.tnv3_tool {
        command.env("TAU_TNV3", path);
    }
    match options.timing {
        FrameTiming::Default => {}
        FrameTiming::RoundDown => {
            command.arg("--fps-round-down");
        }
        FrameTiming::RoundDownEofPass => {
            command.args(["--fps-round-down", "--fps-eof-pass"]);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start C encoder (Python 3 is required): {e}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Could not capture encoder diagnostics")?;
    let thread = std::thread::spawn(move || capture(stderr));
    let stdout = child
        .stdout
        .take()
        .ok_or("Could not capture encoder progress")?;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(part) = line.split("encoded ").nth(1) {
            let mut tokens = part.split_whitespace();
            if let (Some(done), Some("/"), Some(total)) =
                (tokens.next(), tokens.next(), tokens.next())
            {
                if let (Ok(done), Ok(total)) = (done.parse(), total.parse()) {
                    progress(done, total);
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("Could not wait for C encoder: {e}"))?;
    let diagnostics = thread
        .join()
        .map_err(|_| "Encoder diagnostics thread failed")?
        .map_err(|e| format!("Could not read encoder diagnostics: {e}"))?;
    if !status.success() {
        return Err(format!("C encoder failed ({status}). {diagnostics}"));
    }
    let json: serde_json::Value = serde_json::from_reader(
        File::open(video.with_extension("json"))
            .map_err(|e| format!("Missing encoder report: {e}"))?,
    )
    .map_err(|e| format!("Invalid encoder report: {e}"))?;
    let number = |key: &str| -> Result<u32, String> {
        json[key]
            .as_u64()
            .and_then(|x| u32::try_from(x).ok())
            .ok_or_else(|| format!("Invalid encoder report: {key}"))
    };
    let frames = number("frames")?;
    let level = u8::try_from(number("level")?).map_err(|_| "Invalid encoder level")?;
    if json["valid"] != true || frames == 0 || !(1..=3).contains(&level) {
        return Err("Encoder did not report a valid video".into());
    }
    let reencoded = number("pictures_raised_for_level")?;
    io::copy(
        &mut File::open(video).map_err(|e| format!("Missing encoded video: {e}"))?,
        output,
    )
    .map_err(|e| format!("Could not copy encoded video: {e}"))?;
    Ok(Report {
        frames,
        level,
        reencoded,
        diagnostics,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn encoder_temp_directory_has_no_group_or_other_permissions() {
        let temp = temp_dir().unwrap();
        let mode = fs::metadata(&temp.0).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode & 0o077,
            0,
            "encoder temp directory exposes group/other permissions: {mode:o}"
        );
    }
}

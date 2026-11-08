//! Process-based TAC audio decoder: wraps the C reference tools (`tnv3 demux`, then `tac dec` or
//! `tac_fx dec`). No FFI and no audio decoding in Rust; the pure-Rust decoder is parked until the
//! format is closed. Checking an audio payload this way is only as good as the C tools: it is not an
//! independent implementation. Cancellation and timeouts are not provided.
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Which reference decoder to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// `tac dec`: double precision, normative within +-1 sample / 0.3 RMS (spec section 7).
    Reference,
    /// `tac_fx dec`: integer model, the oracle for the hardware decoder (spec section 10).
    Integer,
}

/// Paths of the C tools. `tac` must be the executable that matches the chosen [`Model`].
#[derive(Clone, Debug)]
pub struct Tools {
    pub tnv3: PathBuf,
    pub tac: PathBuf,
}

/// Failure of the wrapper.
#[non_exhaustive]
#[derive(Debug)]
pub enum DecodeError {
    /// Rejected before any process started (for example the output already exists).
    InvalidInput(String),
    /// The file has no audio stream.
    NoAudio,
    Spawn { tool: &'static str, source: io::Error },
    Io { operation: &'static str, source: io::Error },
    /// A tool exited unsuccessfully; `diagnostics` is its (truncated) stderr.
    ToolFailure { tool: &'static str, status: String, diagnostics: String },
}
impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(s) => f.write_str(s),
            Self::NoAudio => f.write_str("The file has no audio stream."),
            Self::Spawn { tool, source } => write!(f, "Could not start {tool}: {source}"),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::ToolFailure { tool, status, diagnostics } => {
                write!(f, "{tool} failed ({status}). {diagnostics}")
            }
        }
    }
}
impl std::error::Error for DecodeError {}

/// What was written.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub channels: u16,
    pub samples_per_channel: u32,
    pub wav_bytes: u64,
}

fn run(tool: &'static str, mut cmd: Command) -> Result<(), DecodeError> {
    let out = cmd
        .output()
        .map_err(|source| DecodeError::Spawn { tool, source })?;
    if out.status.success() {
        return Ok(());
    }
    let mut d = String::from_utf8_lossy(&out.stderr).into_owned();
    d.truncate(d.char_indices().nth(2048).map_or(d.len(), |(i, _)| i));
    Err(DecodeError::ToolFailure { tool, status: out.status.to_string(), diagnostics: d })
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Decodes the audio of `input` (a TNV3 file) to a 16-bit PCM WAV at `output`, which must not exist.
/// The result is published with a no-replace hard link, so an existing file is never overwritten.
pub fn decode_audio(
    tools: &Tools,
    input: &Path,
    output: &Path,
) -> Result<Report, DecodeError> {
    if output.exists() {
        return Err(DecodeError::InvalidInput(format!(
            "Output already exists: {}",
            output.display()
        )));
    }
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = parent.join(format!(
        ".tau-audio-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).map_err(|source| DecodeError::Io { operation: "create workspace", source })?;
    let ws = Workspace(dir);
    let tac = ws.0.join("audio.tac");
    let wav = ws.0.join("audio.wav");
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut demux = Command::new(&tools.tnv3);
    demux.arg("demux").arg(input).arg(null).arg(&tac);
    run("tnv3 demux", demux)?;
    if !tac.is_file() {
        return Err(DecodeError::NoAudio);
    }
    let mut dec = Command::new(&tools.tac);
    dec.arg("dec").arg(&tac).arg(&wav);
    run("TAC decoder", dec)?;
    let bytes = fs::read(&wav).map_err(|source| DecodeError::Io { operation: "read decoded WAV", source })?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[36..40] != b"data" {
        return Err(DecodeError::ToolFailure {
            tool: "TAC decoder",
            status: "ok".into(),
            diagnostics: "The decoder wrote no valid WAV header.".into(),
        });
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let data = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
    if channels == 0 || data as u64 + 44 != bytes.len() as u64 {
        return Err(DecodeError::ToolFailure {
            tool: "TAC decoder",
            status: "ok".into(),
            diagnostics: "The decoder's WAV header does not match its length.".into(),
        });
    }
    fs::hard_link(&wav, output).map_err(|source| DecodeError::Io { operation: "publish WAV (no replace)", source })?;
    Ok(Report {
        channels,
        samples_per_channel: data / (2 * u32::from(channels)),
        wav_bytes: bytes.len() as u64,
    })
}

/// Paths of the tools used to encode: FFmpeg converts any input to 48 kHz PCM, then `tac enc`.
#[derive(Clone, Debug)]
pub struct EncodeTools {
    pub ffmpeg: PathBuf,
    pub tac: PathBuf,
}

/// What was written by [`encode_audio`].
#[derive(Debug, PartialEq, Eq)]
pub struct EncodeReport {
    pub channels: u8,
    pub samples_per_channel: u32,
    pub tac_frames: u32,
    pub bytes: u64,
}

/// Encodes the first audio stream of `input` (any file FFmpeg reads) to a TAC file at `output`, which
/// must not exist. `kbps` is the TAC target bit rate; `channels` is 1 or 2 (more is not supported by
/// TAC and FFmpeg downmixes to this). The encoder is the C reference tool; nothing is encoded in Rust.
pub fn encode_audio(
    tools: &EncodeTools,
    input: &Path,
    output: &Path,
    kbps: u32,
    channels: u8,
) -> Result<EncodeReport, DecodeError> {
    if output.exists() {
        return Err(DecodeError::InvalidInput(format!("Output already exists: {}", output.display())));
    }
    if !(1..=2).contains(&channels) || !(16..=512).contains(&kbps) {
        return Err(DecodeError::InvalidInput("Channels must be 1 or 2 and the bit rate 16..512 kbit/s.".into()));
    }
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = parent.join(format!(".tau-audio-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).map_err(|source| DecodeError::Io { operation: "create workspace", source })?;
    let ws = Workspace(dir);
    let wav = ws.0.join("audio.wav");
    let tac = ws.0.join("audio.tac");
    let mut ff = Command::new(&tools.ffmpeg);
    ff.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-map", "0:a:0", "-vn", "-ar", "48000", "-ac"])
        .arg(channels.to_string())
        .args(["-c:a", "pcm_s16le", "-f", "wav"])
        .arg(&wav);
    run("FFmpeg", ff)?;
    let mut enc = Command::new(&tools.tac);
    enc.arg("enc").arg(&wav).arg(&tac).arg("-b").arg(kbps.to_string());
    run("TAC encoder", enc)?;
    let b = fs::read(&tac).map_err(|source| DecodeError::Io { operation: "read encoded TAC", source })?;
    let u32at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    if b.len() < 20 || &b[..4] != b"TAC0" || u32at(4) != 48000 || b[8] != channels {
        return Err(DecodeError::ToolFailure {
            tool: "TAC encoder",
            status: "ok".into(),
            diagnostics: "The encoder wrote no valid 48 kHz TAC0 header.".into(),
        });
    }
    let report = EncodeReport { channels: b[8], samples_per_channel: u32at(16), tac_frames: u32at(12), bytes: b.len() as u64 };
    fs::hard_link(&tac, output).map_err(|source| DecodeError::Io { operation: "publish TAC (no replace)", source })?;
    Ok(report)
}

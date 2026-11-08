//! Process-based TNV3 muxer: wraps `tnv3 mux` (the C reference tool). It joins an existing TVC
//! video file, an optional TAC audio file and an optional TST subtitle area into a `.tnv3` file.
//! No muxing logic lives in Rust; the C tool validates its own output before writing it.
//! The result is published with a no-replace hard link. No cancellation or timeout.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

/// What to mux. `video` is required; everything else is optional.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub video: PathBuf,
    pub audio: Option<PathBuf>,
    /// A complete TST subtitle area (container spec section 6), not a subtitle text file.
    pub subtitles: Option<PathBuf>,
    pub crc: bool,
    /// Level for a TVC0 input (TVC1 carries its own). `None` lets the tool decide.
    pub level: Option<u8>,
    /// Frame rate override as numerator/denominator.
    pub rate: Option<(u32, u32)>,
    /// Trim audio that is longer than the video.
    pub trim_audio: bool,
    /// Audio interleave shift in TAC frames.
    pub audio_shift: Option<i32>,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum MuxError {
    InvalidInput(String),
    Spawn { source: io::Error },
    Io { operation: &'static str, source: io::Error },
    /// `tnv3 mux` exited unsuccessfully; `diagnostics` is its (truncated) stderr.
    ToolFailure { status: String, diagnostics: String },
}
impl std::fmt::Display for MuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(s) => f.write_str(s),
            Self::Spawn { source } => write!(f, "Could not start tnv3: {source}"),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::ToolFailure { status, diagnostics } => write!(f, "tnv3 mux failed ({status}). {diagnostics}"),
        }
    }
}
impl std::error::Error for MuxError {}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub bytes: u64,
    pub audio: bool,
    pub subtitles: bool,
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Builds the argument list for `tnv3 mux` (separate so it can be tested without the tool).
pub fn mux_args(inputs: &Inputs, out: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = vec!["mux".into(), inputs.video.clone().into()];
    if let Some(p) = &inputs.audio {
        a.push("-a".into());
        a.push(p.clone().into());
    }
    if let Some(p) = &inputs.subtitles {
        a.push("-s".into());
        a.push(p.clone().into());
    }
    if inputs.crc {
        a.push("-c".into());
    }
    if let Some(l) = inputs.level {
        a.push("-L".into());
        a.push(l.to_string().into());
    }
    if let Some((n, d)) = inputs.rate {
        a.push("-r".into());
        a.push(format!("{n}/{d}").into());
    }
    if inputs.trim_audio {
        a.push("-T".into());
    }
    if let Some(s) = inputs.audio_shift {
        a.push("-A".into());
        a.push(s.to_string().into());
    }
    a.push("-o".into());
    a.push(out.into());
    a
}

/// Muxes into `output`, which must not exist.
pub fn mux(tnv3: &Path, inputs: &Inputs, output: &Path) -> Result<Report, MuxError> {
    if output.exists() {
        return Err(MuxError::InvalidInput(format!("Output already exists: {}", output.display())));
    }
    for (what, p) in [("video", Some(&inputs.video)), ("audio", inputs.audio.as_ref()), ("subtitle area", inputs.subtitles.as_ref())] {
        if let Some(p) = p {
            if !p.is_file() {
                return Err(MuxError::InvalidInput(format!("The {what} input is not a file: {}", p.display())));
            }
        }
    }
    if inputs.rate.is_some_and(|(n, d)| n == 0 || d == 0) {
        return Err(MuxError::InvalidInput("The frame rate must be a positive fraction.".into()));
    }
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = parent.join(format!(".tau-mux-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).map_err(|source| MuxError::Io { operation: "create workspace", source })?;
    let ws = Workspace(dir);
    let tmp = ws.0.join("out.tnv3");
    let out = Command::new(tnv3)
        .args(mux_args(inputs, &tmp))
        .output()
        .map_err(|source| MuxError::Spawn { source })?;
    if !out.status.success() {
        let mut d = String::from_utf8_lossy(&out.stderr).into_owned();
        d.truncate(d.char_indices().nth(2048).map_or(d.len(), |(i, _)| i));
        return Err(MuxError::ToolFailure { status: out.status.to_string(), diagnostics: d });
    }
    let bytes = fs::metadata(&tmp)
        .map_err(|source| MuxError::Io { operation: "read muxed file", source })?
        .len();
    if bytes < 64 {
        return Err(MuxError::ToolFailure { status: "ok".into(), diagnostics: "The tool wrote no valid TNV3 file.".into() });
    }
    fs::hard_link(&tmp, output).map_err(|source| MuxError::Io { operation: "publish file (no replace)", source })?;
    Ok(Report { bytes, audio: inputs.audio.is_some(), subtitles: inputs.subtitles.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args_cover_every_option_in_tool_order() {
        let i = Inputs {
            video: "v.tvc".into(),
            audio: Some("a.tac".into()),
            subtitles: Some("s.bin".into()),
            crc: true,
            level: Some(2),
            rate: Some((30000, 1001)),
            trim_audio: true,
            audio_shift: Some(-1),
        };
        let a: Vec<String> = mux_args(&i, Path::new("o")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(a, ["mux", "v.tvc", "-a", "a.tac", "-s", "s.bin", "-c", "-L", "2", "-r", "30000/1001", "-T", "-A", "-1", "-o", "o"]);
        let min: Vec<String> = mux_args(&Inputs { video: "v".into(), ..Default::default() }, Path::new("o"))
            .iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(min, ["mux", "v", "-o", "o"]);
    }
}

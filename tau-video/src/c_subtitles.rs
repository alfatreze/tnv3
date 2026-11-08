//! Subtitle authoring through Tau V's reference builder (`tools/tst_ref/tst_build.py`, needs Python 3
//! and Pillow) and, for text tracks inside a source file, FFmpeg. Nothing is rasterised in Rust; this is
//! a process wrapper like `c_audio` and `c_mux`. The builder's warnings (for example the known
//! "does not fit above the stack; clamped" placement case) are returned, never dropped. Image-based
//! subtitles (PGS, VobSub) cannot be converted. No cancellation or timeout.
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

/// One subtitle track file (SRT or ASS) and its flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSpec {
    pub path: PathBuf,
    pub language: Option<String>,
    pub default: bool,
    pub forced: bool,
}
impl TrackSpec {
    /// The builder's `FILE[:lang[:default][:forced]]` argument. The path must not contain `:`.
    pub fn argument(&self) -> Result<String, SubtitleError> {
        let p = self.path.to_string_lossy();
        if p.contains(':') {
            return Err(SubtitleError::InvalidInput(format!("The builder cannot take a subtitle path containing ':': {p}")));
        }
        let mut s = p.into_owned();
        if self.language.is_some() || self.default || self.forced {
            s.push(':');
            s.push_str(self.language.as_deref().unwrap_or(""));
            if self.default {
                s.push_str(":default");
            }
            if self.forced {
                s.push_str(":forced");
            }
        }
        Ok(s)
    }
}

#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub python: PathBuf,
    pub builder: PathBuf,
    pub tracks: Vec<TrackSpec>,
    /// `[NAME[:STYLE]=]FILE[@SCALE]`, as the builder takes them; the first is the default font.
    pub fonts: Vec<String>,
    /// Cell sizes `WxH,WxH,...` with increasing height (default `20x28`).
    pub sizes: String,
    /// Picture size.
    pub video: (u32, u32),
    pub fps: Option<(u32, u32)>,
    pub styles: Option<PathBuf>,
    /// Baked outline radius 0..4: one value or one per size, comma separated.
    pub outline_width: Option<String>,
    /// Any further builder options, passed through verbatim.
    pub extra: Vec<OsString>,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum SubtitleError {
    InvalidInput(String),
    Spawn { tool: &'static str, source: io::Error },
    Io { operation: &'static str, source: io::Error },
    ToolFailure { tool: &'static str, status: String, diagnostics: String },
}
impl std::fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(s) => f.write_str(s),
            Self::Spawn { tool, source } => write!(f, "Could not start {tool}: {source}"),
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::ToolFailure { tool, status, diagnostics } => write!(f, "{tool} failed ({status}). {diagnostics}"),
        }
    }
}
impl std::error::Error for SubtitleError {}

#[derive(Debug, PartialEq, Eq)]
pub struct BuildReport {
    pub bytes: u64,
    /// Builder stderr lines that mention a warning (each is a real placement or input problem).
    pub warnings: Vec<String>,
    /// Builder stdout (one line per track and size, then the area size).
    pub summary: Vec<String>,
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn workspace(parent: &Path) -> Result<Workspace, SubtitleError> {
    let dir = parent.join(format!(".tau-subs-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).map_err(|source| SubtitleError::Io { operation: "create workspace", source })?;
    Ok(Workspace(dir))
}
fn truncated(bytes: &[u8]) -> String {
    let mut d = String::from_utf8_lossy(bytes).into_owned();
    d.truncate(d.char_indices().nth(2048).map_or(d.len(), |(i, _)| i));
    d
}

/// The builder command line (separate so it can be tested without Python).
pub fn build_args(o: &BuildOptions, out: &Path) -> Result<Vec<OsString>, SubtitleError> {
    if o.tracks.is_empty() {
        return Err(SubtitleError::InvalidInput("At least one subtitle track is required.".into()));
    }
    if o.fonts.is_empty() {
        return Err(SubtitleError::InvalidInput("At least one font is required.".into()));
    }
    if o.video.0 == 0 || o.video.1 == 0 {
        return Err(SubtitleError::InvalidInput("The picture size must be positive.".into()));
    }
    let mut a: Vec<OsString> = vec![o.builder.clone().into()];
    for t in &o.tracks {
        a.push("--track".into());
        a.push(t.argument()?.into());
    }
    a.push("--sizes".into());
    a.push(o.sizes.clone().into());
    for f in &o.fonts {
        a.push("--font".into());
        a.push(f.clone().into());
    }
    if let Some(s) = &o.styles {
        a.push("--styles".into());
        a.push(s.clone().into());
    }
    a.push("--video".into());
    a.push(format!("{}x{}", o.video.0, o.video.1).into());
    if let Some((n, d)) = o.fps {
        a.push("--fps".into());
        a.push(format!("{n}/{d}").into());
    }
    if let Some(w) = &o.outline_width {
        a.push("--outline-width".into());
        a.push(w.clone().into());
    }
    a.extend(o.extra.iter().cloned());
    a.push("-o".into());
    a.push(out.into());
    Ok(a)
}

/// Builds a subtitle area at `output`, which must not exist.
pub fn build_area(o: &BuildOptions, output: &Path) -> Result<BuildReport, SubtitleError> {
    if output.exists() {
        return Err(SubtitleError::InvalidInput(format!("Output already exists: {}", output.display())));
    }
    if !o.builder.is_file() {
        return Err(SubtitleError::InvalidInput(format!("The subtitle builder was not found: {}", o.builder.display())));
    }
    for t in &o.tracks {
        if !t.path.is_file() {
            return Err(SubtitleError::InvalidInput(format!("Subtitle file not found: {}", t.path.display())));
        }
    }
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let ws = workspace(parent)?;
    let tmp = ws.0.join("area.bin");
    let args = build_args(o, &tmp)?;
    let out = Command::new(&o.python)
        .args(&args)
        .output()
        .map_err(|source| SubtitleError::Spawn { tool: "Python", source })?;
    if !out.status.success() {
        return Err(SubtitleError::ToolFailure {
            tool: "subtitle builder",
            status: out.status.to_string(),
            diagnostics: truncated(&out.stderr),
        });
    }
    let bytes = fs::metadata(&tmp)
        .map_err(|source| SubtitleError::Io { operation: "read subtitle area", source })?
        .len();
    if bytes == 0 {
        return Err(SubtitleError::ToolFailure { tool: "subtitle builder", status: "ok".into(), diagnostics: "The builder wrote an empty area.".into() });
    }
    fs::hard_link(&tmp, output).map_err(|source| SubtitleError::Io { operation: "publish area (no replace)", source })?;
    let lines = |b: &[u8]| String::from_utf8_lossy(b).lines().map(str::to_owned).collect::<Vec<_>>();
    Ok(BuildReport {
        bytes,
        warnings: lines(&out.stderr).into_iter().filter(|l| l.to_ascii_lowercase().contains("warning")).collect(),
        summary: lines(&out.stdout),
    })
}

/// Extracts the `track`-th (zero-based) subtitle stream of `input` as text into `output` (which must not
/// exist), in `format` `"ass"` (keeps styles, fonts, colours, italics and bold; the intermediate format of
/// choice) or `"srt"` (plain text with simple tags). Fails for image-based subtitles.
pub fn extract_text_subtitle(ffmpeg: &Path, input: &Path, track: u32, output: &Path, format: &str) -> Result<(), SubtitleError> {
    if !matches!(format, "ass" | "srt") {
        return Err(SubtitleError::InvalidInput("The subtitle text format is ass or srt.".into()));
    }
    if output.exists() {
        return Err(SubtitleError::InvalidInput(format!("Output already exists: {}", output.display())));
    }
    let out = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .arg("-map")
        .arg(format!("0:s:{track}"))
        .args(["-c:s", format, "-f", format])
        .arg(output)
        .output()
        .map_err(|source| SubtitleError::Spawn { tool: "FFmpeg", source })?;
    if !out.status.success() {
        let _ = fs::remove_file(output);
        let mut d = truncated(&out.stderr);
        d.push_str(" (Only text subtitles can be converted; image subtitles such as PGS or VobSub cannot.)");
        return Err(SubtitleError::ToolFailure { tool: "FFmpeg subtitle extraction", status: out.status.to_string(), diagnostics: d });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opts() -> BuildOptions {
        BuildOptions {
            python: "python3".into(),
            builder: "tst_build.py".into(),
            tracks: vec![
                TrackSpec { path: "a.srt".into(), language: Some("eng".into()), default: true, forced: false },
                TrackSpec { path: "b.ass".into(), language: None, default: false, forced: true },
            ],
            fonts: vec!["Roboto-Regular.ttf".into()],
            sizes: "16x24,20x28".into(),
            video: (800, 334),
            fps: Some((24, 1)),
            styles: None,
            outline_width: Some("2".into()),
            extra: vec!["--align".into(), "block".into()],
        }
    }
    #[test]
    fn track_arguments_follow_the_builder_syntax() {
        let o = opts();
        assert_eq!(o.tracks[0].argument().unwrap(), "a.srt:eng:default");
        assert_eq!(o.tracks[1].argument().unwrap(), "b.ass::forced");
        let plain = TrackSpec { path: "c.srt".into(), language: None, default: false, forced: false };
        assert_eq!(plain.argument().unwrap(), "c.srt");
        assert!(TrackSpec { path: "C:/x.srt".into(), language: None, default: false, forced: false }.argument().is_err());
    }
    #[test]
    fn builder_command_line_is_complete_and_ordered() {
        let a: Vec<String> = build_args(&opts(), Path::new("o.bin")).unwrap().iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(a, ["tst_build.py", "--track", "a.srt:eng:default", "--track", "b.ass::forced", "--sizes", "16x24,20x28",
            "--font", "Roboto-Regular.ttf", "--video", "800x334", "--fps", "24/1", "--outline-width", "2", "--align", "block", "-o", "o.bin"]);
        let mut bad = opts();
        bad.tracks.clear();
        assert!(build_args(&bad, Path::new("o")).is_err());
        let mut bad = opts();
        bad.fonts.clear();
        assert!(build_args(&bad, Path::new("o")).is_err());
        let mut bad = opts();
        bad.video = (0, 10);
        assert!(build_args(&bad, Path::new("o")).is_err());
    }
}

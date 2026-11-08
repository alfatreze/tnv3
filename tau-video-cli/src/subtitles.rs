//! Shared subtitle authoring for `build-subtitles` and `encode --subtitle-*`: collects text tracks
//! (files, or text streams extracted from the source with FFmpeg), then runs Tau V's reference builder
//! through `tau_video::c_subtitles`. Needs Python 3 with Pillow, the builder script and a font.
use super::*;
use std::fs;
use tau_video::c_subtitles::{self, BuildOptions, BuildReport, TrackSpec};

/// What the user asked for; paths are resolved by [`build`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub files: Vec<TrackSpec>,
    /// Zero-based subtitle stream index in the source, with flags (`path` unused).
    pub source_tracks: Vec<(u32, TrackSpec)>,
    pub fonts: Vec<String>,
    pub sizes: Option<String>,
    pub outline_width: Option<String>,
    pub styles: Option<PathBuf>,
    pub extra: Vec<OsString>,
    pub builder: Option<PathBuf>,
    pub python: Option<PathBuf>,
}

impl Request {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.source_tracks.is_empty()
    }
}

/// Parses `FILE[:lang[:default][:forced]]` (flags only after the first colon).
pub fn parse_file_track(text: &str) -> Result<TrackSpec, String> {
    let mut parts = text.split(':');
    let path = parts.next().unwrap_or("");
    if path.is_empty() {
        return Err("A subtitle file name is required.".into());
    }
    let mut spec = TrackSpec { path: PathBuf::from(path), language: None, default: false, forced: false };
    apply_flags(&mut spec, parts)?;
    Ok(spec)
}

/// Parses `N[:lang[:default][:forced]]` for a stream of the source.
pub fn parse_source_track(text: &str) -> Result<(u32, TrackSpec), String> {
    let mut parts = text.split(':');
    let index: u32 = parts
        .next()
        .and_then(|n| n.parse().ok())
        .filter(|n| *n < 128)
        .ok_or("A subtitle track is a number from 0 to 127 (0 = first subtitle stream).")?;
    let mut spec = TrackSpec { path: PathBuf::new(), language: None, default: false, forced: false };
    apply_flags(&mut spec, parts)?;
    Ok((index, spec))
}

fn apply_flags<'a>(spec: &mut TrackSpec, parts: impl Iterator<Item = &'a str>) -> Result<(), String> {
    for (i, p) in parts.enumerate() {
        match (i, p) {
            (_, "default") => spec.default = true,
            (_, "forced") => spec.forced = true,
            (0, lang) if !lang.is_empty() => spec.language = Some(lang.to_owned()),
            (0, _) => {}
            _ => return Err(format!("Unknown subtitle track flag: {p} (use default or forced).")),
        }
    }
    Ok(())
}

/// Exit-code class of a builder failure: usage 2, tool 1, environment 3.
fn classify(e: c_subtitles::SubtitleError) -> (u8, String) {
    use c_subtitles::SubtitleError as E;
    let code = match &e {
        E::InvalidInput(_) => 2,
        E::ToolFailure { .. } => 1,
        E::Spawn { .. } | E::Io { .. } => 3,
        _ => 1,
    };
    (code, e.to_string())
}

fn find_builder(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let found = explicit
        .map(Path::to_path_buf)
        .or_else(|| env::var_os("TAU_TST_BUILD").map(PathBuf::from))
        .or_else(|| {
            env::var_os("TAU_V_ROOT").map(|r| PathBuf::from(r).join("tools/tst_ref/tst_build.py"))
        });
    match found {
        Some(p) if p.is_file() => Ok(p),
        Some(p) => Err(format!("The subtitle builder was not found: {}", p.display())),
        None => Err("No subtitle builder: pass --subtitle-builder (or --builder), or set TAU_TST_BUILD to Tau V's tools/tst_ref/tst_build.py.".into()),
    }
}

/// Builds the area at `out`. `workdir` must exist and is used for extracted text; files created there
/// are removed before returning. `source` and `ffmpeg` are needed only for source tracks.
pub fn build(
    req: &Request,
    source: Option<&Path>,
    ffmpeg: Option<&Path>,
    video: (u32, u32),
    fps: Option<(u32, u32)>,
    workdir: &Path,
    out: &Path,
) -> Result<BuildReport, (u8, String)> {
    let builder = find_builder(req.builder.as_deref()).map_err(|e| (2, e))?;
    let python = req
        .python
        .clone()
        .map(resolve_executable_arg)
        .or_else(|| on_path("python3"))
        .or_else(|| on_path("python"))
        .ok_or((3, "Python 3 was not found; pass --python.".to_owned()))?;
    let fonts = if req.fonts.is_empty() {
        let default = builder.parent().map(|d| d.join("fonts/Roboto-Regular.ttf")).filter(|p| p.is_file());
        match default {
            Some(p) => vec![p.display().to_string()],
            None => return Err((2, "A subtitle font is required: pass --font FILE.ttf (the builder's fonts folder has no default here).".into())),
        }
    } else {
        req.fonts.clone()
    };
    let mut tracks = req.files.clone();
    let mut extracted = Vec::new();
    let result: Result<BuildReport, (u8, String)> = (|| {
        for (n, (index, flags)) in req.source_tracks.iter().enumerate() {
            let (Some(source), Some(ffmpeg)) = (source, ffmpeg) else {
                return Err((2, "Subtitle tracks from the source need the source file and FFmpeg.".to_owned()));
            };
            let file = workdir.join(format!("subtitle-{n}.srt"));
            c_subtitles::extract_text_subtitle(ffmpeg, source, *index, &file).map_err(classify)?;
            extracted.push(file.clone());
            tracks.push(TrackSpec { path: file, ..flags.clone() });
        }
        c_subtitles::build_area(
            &BuildOptions {
                python,
                builder,
                tracks,
                fonts,
                sizes: req.sizes.clone().unwrap_or_else(|| "20x28".into()),
                video,
                fps,
                styles: req.styles.clone(),
                outline_width: req.outline_width.clone(),
                extra: req.extra.clone(),
            },
            out,
        )
        .map_err(classify)
    })();
    for f in extracted {
        let _ = fs::remove_file(f);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn track_flags_parse_and_reject_unknowns() {
        let t = parse_file_track("a.srt:eng:default:forced").unwrap();
        assert_eq!((t.language.as_deref(), t.default, t.forced), (Some("eng"), true, true));
        let t = parse_file_track("a.srt::forced").unwrap();
        assert_eq!((t.language, t.forced), (None, true));
        assert!(parse_file_track("").is_err());
        assert!(parse_file_track("a.srt:eng:loud").is_err());
        let (i, t) = parse_source_track("2:jpn").unwrap();
        assert_eq!((i, t.language.as_deref()), (2, Some("jpn")));
        assert!(parse_source_track("x").is_err());
        assert!(parse_source_track("200").is_err());
    }
}

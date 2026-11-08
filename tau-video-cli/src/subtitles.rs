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
fn build_with_python(
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

/// Whether the legacy Python builder was asked for (explicit path or `TAU_TST_BUILD`).
fn wants_python(req: &Request) -> bool {
    req.builder.is_some() || env::var_os("TAU_TST_BUILD").is_some()
}

/// Builds the area at `out`: natively in Rust by default; with Tau V's Python builder only when a builder
/// path is given (`--builder`, `--subtitle-builder` or `TAU_TST_BUILD`). `workdir` must exist.
pub fn build(
    req: &Request,
    source: Option<&Path>,
    ffmpeg: Option<&Path>,
    video: (u32, u32),
    fps: Option<(u32, u32)>,
    workdir: &Path,
    out: &Path,
) -> Result<BuildReport, (u8, String)> {
    if wants_python(req) {
        build_with_python(req, source, ffmpeg, video, fps, workdir, out)
    } else {
        build_native(req, source, ffmpeg, video, fps, workdir, out)
    }
}

fn find_font() -> Option<String> {
    env::var_os("TAU_SUBTITLE_FONT").map(PathBuf::from).filter(|p| p.is_file()).map(|p| p.display().to_string())
}

fn parse_sizes(s: &str) -> Result<Vec<(u32, u32)>, String> {
    s.split(',')
        .map(|p| {
            let (w, h) = p.split_once('x').ok_or("sizes must be WxH,WxH,...")?;
            Ok((w.parse().map_err(|_| "sizes must be WxH,WxH,...")?, h.parse().map_err(|_| "sizes must be WxH,WxH,...")?))
        })
        .collect()
}

fn apply_extra(o: &mut tau_video::subtitle::Options, extra: &[OsString]) -> Result<(), String> {
    use tau_video::subtitle::{Align, Placement};
    let mut it = extra.iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(k) = it.next() {
        let mut val = |what: &str| it.next().ok_or_else(|| format!("{what} needs a value"));
        match k.as_str() {
            "--align" => o.align = match val("--align")?.as_str() { "center" => Align::Center, "block" => Align::Block, _ => return Err("--align is center or block".into()) },
            "--placement" => o.placement = match val("--placement")?.as_str() { "auto" => Placement::Auto, "bar" => Placement::Bar, "picture" => Placement::Picture, _ => return Err("--placement is auto, bar or picture".into()) },
            "--margin" => o.margin = val("--margin")?.parse().map_err(|_| "--margin is a number")?,
            "--bottom" => o.bottom = val("--bottom")?.parse().map_err(|_| "--bottom is a number")?,
            "--font-size" => o.font_size = Some(val("--font-size")?.parse().map_err(|_| "--font-size is a number")?),
            "--italic-color" => {
                let v = val("--italic-color")?;
                let n = u32::from_str_radix(&v, 16).ok().filter(|_| v.len() == 6).ok_or("--italic-color is RRGGBB")?;
                o.italic_rgb = ((n >> 16) as u8, (n >> 8) as u8, n as u8);
            }
            "--no-center-single" => o.center_single = false,
            "--no-sdh" => o.sdh = false,
            other => return Err(format!("unknown subtitle option {other} (native builder knows --align, --placement, --margin, --bottom, --font-size, --italic-color, --no-center-single, --no-sdh)")),
        }
    }
    Ok(())
}

fn load_styles(path: &Path) -> Result<std::collections::BTreeMap<String, tau_video::subtitle::SpeakerStyle>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("styles file {}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("styles file: {e}"))?;
    let obj = v.as_object().ok_or("styles file must be a JSON object")?;
    let mut out = std::collections::BTreeMap::new();
    for (k, s) in obj {
        let o = s.as_object().filter(|o| !k.is_empty() && o.keys().all(|k| matches!(k.as_str(), "font" | "color" | "bold" | "italic")))
            .ok_or_else(|| format!("styles file: entry {k:?} needs an object with font/color/bold/italic"))?;
        out.insert(k.clone(), tau_video::subtitle::SpeakerStyle {
            font: o.get("font").and_then(|x| x.as_str()).map(str::to_owned),
            color: o.get("color").and_then(|x| x.as_str()).map(str::to_owned),
            bold: o.get("bold").and_then(|x| x.as_bool()),
            italic: o.get("italic").and_then(|x| x.as_bool()),
        });
    }
    Ok(out)
}

fn build_native(
    req: &Request,
    source: Option<&Path>,
    ffmpeg: Option<&Path>,
    video: (u32, u32),
    fps: Option<(u32, u32)>,
    workdir: &Path,
    out: &Path,
) -> Result<BuildReport, (u8, String)> {
    use tau_video::subtitle::{self, FontSpec, TrackInput};
    let usage = |e: String| (2u8, e);
    // Fonts: those given; else TAU_SUBTITLE_FONT; else the built-in Roboto Regular + Italic.
    let fonts = if !req.fonts.is_empty() {
        req.fonts.iter().map(|a| FontSpec::from_argument(a.as_str()).map_err(|e| usage(e.to_string()))).collect::<Result<Vec<_>, _>>()?
    } else if let Some(p) = find_font() {
        vec![FontSpec::from_argument(&p).map_err(|e| usage(e.to_string()))?]
    } else {
        subtitle::default_fonts()
    };
    let mut tracks = Vec::new();
    let mut extracted = Vec::new();
    let result = (|| {
        let mut specs: Vec<(PathBuf, tau_video::c_subtitles::TrackSpec)> = req.files.iter().map(|t| (t.path.clone(), t.clone())).collect();
        for (n, (index, flags)) in req.source_tracks.iter().enumerate() {
            let (Some(source), Some(ffmpeg)) = (source, ffmpeg) else {
                return Err((2, "Subtitle tracks from the source need the source file and FFmpeg.".to_owned()));
            };
            let file = workdir.join(format!("subtitle-{n}.srt"));
            tau_video::c_subtitles::extract_text_subtitle(ffmpeg, source, *index, &file).map_err(classify)?;
            extracted.push(file.clone());
            specs.push((file, flags.clone()));
        }
        for (path, flags) in specs {
            let bytes = fs::read(&path).map_err(|e| (3u8, format!("Subtitle file {}: {e}", path.display())))?;
            let text = String::from_utf8(bytes).map_err(|_| (2u8, format!("Subtitle file {} is not UTF-8.", path.display())))?;
            tracks.push(TrackInput {
                name: path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
                text,
                language: flags.language.clone().unwrap_or_else(|| "und".into()),
                default: flags.default,
                forced: flags.forced,
            });
        }
        let mut o = subtitle::Options::new(tracks, fonts, video);
        o.fps = fps;
        if let Some(sz) = &req.sizes {
            o.cells = parse_sizes(sz).map_err(usage)?;
        }
        if let Some(w) = &req.outline_width {
            o.outline_widths = w.split(',').map(|x| x.parse::<u8>().map_err(|_| usage("--outline-width is W or W,W,... (0..4)".into()))).collect::<Result<_, _>>()?;
        }
        if let Some(p) = &req.styles {
            o.styles = load_styles(p).map_err(usage)?;
        }
        apply_extra(&mut o, &req.extra).map_err(usage)?;
        let (area, info) = subtitle::build_area(&o).map_err(|e| (1u8, format!("Subtitle builder: {e}")))?;
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(out).map_err(|e| (if e.kind() == std::io::ErrorKind::AlreadyExists { 2 } else { 3 }, format!("Output {}: {e}", out.display())))?;
        use std::io::Write;
        if let Err(e) = f.write_all(&area).and_then(|_| f.sync_all()) {
            drop(f);
            let _ = fs::remove_file(out);
            return Err((3, format!("Could not write the subtitle area: {e}")));
        }
        let mut summary = Vec::new();
        for t in &info.tracks {
            for v in &t.variants {
                summary.push(format!(
                    "{} {}x{}: {} events ({} dropped), {} records ({} sdh, {} forced, {} colour switches), glyphs {} ({} B, {} faces), events {} B, font {:.2} px, {}, max lines {}, {} colours",
                    t.name, v.cell.0, v.cell.1, t.events_in, t.dropped, v.events_out, v.sdh_events, v.forced_events, v.switches,
                    v.glyphs, v.glyph_bytes, v.faces, v.sub_bytes, v.size, v.region, v.max_lines, t.colours
                ));
            }
        }
        summary.push(format!("area {} B", info.bytes));
        Ok(BuildReport { bytes: info.bytes as u64, warnings: info.warnings, summary })
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

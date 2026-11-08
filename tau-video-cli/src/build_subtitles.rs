//! `build-subtitles OUTPUT.bin ...`: builds a TST subtitle area from SRT/ASS files or text streams of a
//! source, through Tau V's reference builder. Feed the result to `mux --subtitles`.
use super::*;
use std::fs;
use super::subtitles::{self, Request};

const USAGE: &str = "Usage: tau-codecs-cli build-subtitles OUTPUT.bin --video WxH (--track FILE[:lang[:default][:forced]] | --from-source INPUT --source-track N[:lang[:default][:forced]])... [--font [NAME[:STYLE]=]FILE.ttf[@SCALE]]... [--sizes WxH,...] [--fps N/D] [--styles FILE.json] [--outline-width W[,W...]] [--builder-arg ARG]... [--builder PATH] [--python PATH] [--ffmpeg PATH|NAME] [--json]";

fn fail(code: u8, message: &str, json: bool) -> ExitCode {
    if json {
        let _ = write_stdout_line(&serde_json::json!({"command":"build-subtitles","schema":1,"status":"error","code":code,"message":message}).to_string());
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(code)
}

pub fn parse_wh(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0 && w <= 800 && h <= 720).then_some((w, h))
}
pub fn parse_fps(s: &str) -> Option<(u32, u32)> {
    let (n, d) = s.split_once('/')?;
    let (n, d) = (n.parse().ok()?, d.parse().ok()?);
    (n > 0 && d > 0).then_some((n, d))
}

pub fn run(argv: &[OsString]) -> ExitCode {
    let json = argv.iter().any(|a| a == OsStr::new("--json"));
    let mut req = Request::default();
    let (mut video, mut fps, mut source, mut ffmpeg) = (None, None, None::<PathBuf>, None::<PathBuf>);
    let mut positional = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let key = argv[i].to_str().unwrap_or("");
        if key == "--json" {
            i += 1;
            continue;
        }
        if key.starts_with("--") {
            let Some(v) = argv.get(i + 1) else { return fail(2, USAGE, json) };
            i += 1;
            let text = v.to_str().unwrap_or("");
            match key {
                "--video" => match subtitles_wh(text) { Some(x) => video = Some(x), None => return fail(2, USAGE, json) },
                "--fps" => match parse_fps(text) { Some(x) => fps = Some(x), None => return fail(2, USAGE, json) },
                "--track" => match subtitles::parse_file_track(text) { Ok(t) => req.files.push(t), Err(e) => return fail(2, &e, json) },
                "--source-track" => match subtitles::parse_source_track(text) { Ok(t) => req.source_tracks.push(t), Err(e) => return fail(2, &e, json) },
                "--from-source" => source = Some(PathBuf::from(v)),
                "--font" => req.fonts.push(text.to_owned()),
                "--sizes" => req.sizes = Some(text.to_owned()),
                "--outline-width" => req.outline_width = Some(text.to_owned()),
                "--styles" => req.styles = Some(PathBuf::from(v)),
                "--builder-arg" => req.extra.push(v.clone()),
                "--builder" => req.builder = Some(PathBuf::from(v)),
                "--python" => req.python = Some(PathBuf::from(v)),
                "--ffmpeg" => ffmpeg = Some(PathBuf::from(v)),
                _ => return fail(2, USAGE, json),
            }
        } else {
            positional.push(PathBuf::from(&argv[i]));
        }
        i += 1;
    }
    let ([output], Some(video)) = (positional.as_slice(), video) else { return fail(2, USAGE, json) };
    if req.is_empty() {
        return fail(2, "Give at least one --track or --source-track.", json);
    }
    if !req.source_tracks.is_empty() && source.is_none() {
        return fail(2, "--source-track needs --from-source INPUT.", json);
    }
    let ffmpeg = ffmpeg
        .map(resolve_executable_arg)
        .or_else(|| adjacent_executable("ffmpeg"))
        .or_else(|| on_path("ffmpeg"));
    if output.exists() {
        return fail(2, &format!("Output already exists: {}", output.display()), json);
    }
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let work = parent.join(format!(".tau-subs-extract-{}", std::process::id()));
    if let Err(e) = fs::create_dir(&work) {
        return fail(3, &format!("Could not create a working folder: {e}"), json);
    }
    let result = subtitles::build(&req, source.as_deref(), ffmpeg.as_deref(), video, fps, &work, output);
    let _ = fs::remove_dir_all(&work);
    match result {
        Ok(r) => {
            let line = if json {
                serde_json::json!({"command":"build-subtitles","schema":1,"status":"ok","bytes":r.bytes,
                    "warnings":r.warnings,"summary":r.summary,"output":output}).to_string()
            } else {
                let mut s = r.summary.join("\n");
                for w in &r.warnings {
                    s.push_str(&format!("\nWarning: {w}"));
                }
                s
            };
            match write_stdout_line(&line) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(3, &format!("Could not write output: {e}"), false),
            }
        }
        Err((code, e)) => fail(code, &e, json),
    }
}

fn subtitles_wh(s: &str) -> Option<(u32, u32)> {
    parse_wh(s)
}

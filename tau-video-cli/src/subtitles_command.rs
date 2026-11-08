//! `subtitles FILE.tnv3`: reads the subtitle area with the Rust reader and prints tracks, variants, events, or
//! draws one picture of a variant at a time as a PPM file (reference drawing of spec 6.3, no player parameters).
use super::*;
use std::{fs, io::Write};
use tau_video::{
    Tnv3Reader,
    subtitle_read::{GRID_H, GRID_W, GlyphRef, SubtitleArea},
};

const USAGE: &str = "Usage: tau-codecs-cli subtitles FILE.tnv3 [--json] [--events] [--render SECONDS OUT.ppm] [--track N] [--variant N]";

fn fail(code: u8, message: &str, json: bool) -> ExitCode {
    if json {
        let _ = write_stdout_line(&serde_json::json!({"command":"subtitles","schema":1,"status":"error","code":code,"message":message}).to_string());
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(code)
}

fn secs(samples: u32) -> f64 {
    f64::from(samples) / 48000.0
}

fn refs_json(refs: &[GlyphRef]) -> Vec<serde_json::Value> {
    refs.iter()
        .map(|r| match r {
            GlyphRef::Glyph(g) => serde_json::json!(g),
            GlyphRef::Colour(k) => serde_json::json!(format!("colour:{k}")),
            GlyphRef::LineBreak => serde_json::json!("break"),
        })
        .collect()
}

fn summary(area: &SubtitleArea) -> serde_json::Value {
    serde_json::json!({"tracks": area.tracks.iter().enumerate().map(|(i, t)| serde_json::json!({
        "track": i, "language": t.language, "default": t.default, "forced_track": t.forced_track,
        "palette": t.palette.iter().map(|c| format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])).collect::<Vec<_>>(),
        "variants": t.variants.iter().enumerate().map(|(j, v)| serde_json::json!({
            "variant": j, "cell_w": v.cell_w, "cell_h": v.cell_h, "outline_w": v.outline_w,
            "events": v.events.len(), "glyphs": v.glyphs.len(),
            "first_start_seconds": v.events.first().map(|e| secs(e.start)),
            "last_end_seconds": v.events.iter().map(|e| e.end).max().map(secs),
            "sdh_events": v.events.iter().filter(|e| e.sdh).count(),
            "forced_events": v.events.iter().filter(|e| e.forced).count(),
        })).collect::<Vec<_>>(),
    })).collect::<Vec<_>>()})
}

pub fn run(argv: &[OsString]) -> ExitCode {
    let json = argv.iter().any(|a| a == OsStr::new("--json"));
    let (mut file, mut events, mut render, mut track, mut variant) = (None::<PathBuf>, false, None::<(f64, PathBuf)>, 0usize, 0usize);
    let mut i = 1;
    while i < argv.len() {
        match argv[i].to_str().unwrap_or("") {
            "--json" => {}
            "--events" => events = true,
            "--track" | "--variant" => {
                let k = argv[i].to_str().unwrap_or("").to_owned();
                let Some(n) = argv.get(i + 1).and_then(|v| v.to_str()).and_then(|s| s.parse::<usize>().ok()) else { return fail(2, USAGE, json) };
                if k == "--track" { track = n } else { variant = n }
                i += 1;
            }
            "--render" => {
                let (Some(t), Some(o)) = (argv.get(i + 1).and_then(|v| v.to_str()).and_then(|s| s.parse::<f64>().ok()).filter(|t| t.is_finite() && *t >= 0.0), argv.get(i + 2)) else {
                    return fail(2, USAGE, json);
                };
                render = Some((t, PathBuf::from(o)));
                i += 2;
            }
            s if s.starts_with("--") => return fail(2, USAGE, json),
            _ if file.is_none() => file = Some(PathBuf::from(&argv[i])),
            _ => return fail(2, USAGE, json),
        }
        i += 1;
    }
    let Some(file) = file else { return fail(2, USAGE, json) };
    let reader = match fs::File::open(&file) {
        Ok(f) => f,
        Err(e) => return fail(3, &format!("Could not open input: {e}"), json),
    };
    let area = match Tnv3Reader::open(reader).and_then(|mut r| r.read_subtitle_area()) {
        Ok(a) => a,
        Err(tau_video::Error::Io(e)) => return fail(3, &format!("Could not read input: {e}"), json),
        Err(e) => return fail(1, &format!("Invalid file: {e}"), json),
    };
    let Some(area) = area else {
        let line = if json { serde_json::json!({"command":"subtitles","schema":1,"status":"ok","has_subtitles":false}).to_string() } else { "No subtitle area.".to_owned() };
        return match write_stdout_line(&line) { Ok(()) => ExitCode::SUCCESS, Err(e) => fail(3, &format!("Could not write output: {e}"), false) };
    };
    let Some(v) = area.tracks.get(track).and_then(|t| t.variants.get(variant)) else {
        return fail(2, &format!("No track {track} variant {variant} in this file."), json);
    };
    if let Some((seconds, out)) = &render {
        let p = (seconds * 48000.0).round() as u32;
        let frame = v.render(p);
        let pal = &area.tracks[track].palette;
        let mut ppm = format!("P6\n{GRID_W} {GRID_H}\n255\n").into_bytes();
        for k in 0..GRID_W * GRID_H {
            let mut px = [24f32, 24.0, 24.0];
            let o = f32::from(frame.outline[k]) / 3.0;
            for c in &mut px {
                *c *= 1.0 - o;
            }
            let t = f32::from(frame.text[k]) / 3.0;
            let col = pal[usize::from(frame.colour[k] & 15)];
            for (c, s) in px.iter_mut().zip(col) {
                *c = *c * (1.0 - t) + f32::from(s) * t;
            }
            ppm.extend(px.map(|c| c.round() as u8));
        }
        let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(out) {
            Ok(f) => f,
            Err(e) => return fail(if e.kind() == std::io::ErrorKind::AlreadyExists { 2 } else { 3 }, &format!("Output {}: {e}", out.display()), json),
        };
        if let Err(e) = f.write_all(&ppm).and_then(|_| f.sync_all()) {
            drop(f);
            let _ = fs::remove_file(out);
            return fail(3, &format!("Could not write the picture: {e}"), json);
        }
    }
    let mut value = summary(&area);
    value["command"] = "subtitles".into();
    value["schema"] = 1.into();
    value["status"] = "ok".into();
    value["has_subtitles"] = true.into();
    if events {
        value["events"] = v.events.iter().map(|e| serde_json::json!({
            "start": e.start, "end": e.end, "start_seconds": secs(e.start), "end_seconds": secs(e.end),
            "x": e.x, "y": e.y, "colour": e.colour, "forced": e.forced, "sdh": e.sdh, "refs": refs_json(&e.refs),
        })).collect::<Vec<_>>().into();
        value["events_of"] = serde_json::json!({"track": track, "variant": variant});
    }
    if let Some((seconds, out)) = &render {
        value["rendered"] = serde_json::json!({"seconds": seconds, "output": out, "track": track, "variant": variant,
            "active_events": v.active((seconds * 48000.0).round() as u32).count()});
    }
    let line = if json {
        value.to_string()
    } else {
        let mut s = String::new();
        for t in value["tracks"].as_array().into_iter().flatten() {
            s.push_str(&format!("track {} {} default={} forced_track={}\n", t["track"], t["language"], t["default"], t["forced_track"]));
            for var in t["variants"].as_array().into_iter().flatten() {
                s.push_str(&format!("  variant {}: cell {}x{} outline {} events {} glyphs {} sdh {} forced {}\n", var["variant"], var["cell_w"], var["cell_h"], var["outline_w"], var["events"], var["glyphs"], var["sdh_events"], var["forced_events"]));
            }
        }
        if events {
            for e in v.events.iter() {
                s.push_str(&format!("{:.3}-{:.3}s at {},{} colour {}{}{} refs {}\n", secs(e.start), secs(e.end), e.x, e.y, e.colour, if e.sdh { " sdh" } else { "" }, if e.forced { " forced" } else { "" }, e.refs.len()));
            }
        }
        if let Some((seconds, out)) = &render {
            s.push_str(&format!("Drew {} active event(s) at {seconds} s into {}\n", v.active((seconds * 48000.0).round() as u32).count(), out.display()));
        }
        s.trim_end().to_owned()
    };
    match write_stdout_line(&line) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(3, &format!("Could not write output: {e}"), false),
    }
}

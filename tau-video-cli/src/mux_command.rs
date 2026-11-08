//! `mux VIDEO.tvc OUTPUT.tnv3 [--audio A.tac] [--subtitles AREA.bin] ...`: joins existing streams
//! through the C reference muxer (see `tau_video::c_mux`). The inputs must already be encoded; this
//! does not encode, and `--subtitles` takes a finished TST area, not subtitle text.
use super::*;
use std::{fs, path::Path};
use tau_video::c_mux::{self, Inputs, MuxError};

const USAGE: &str = "Usage: tau-codecs-cli mux VIDEO.tvc OUTPUT.tnv3 [--audio FILE.tac] [--subtitles AREA.bin] [--crc] [--level 1..3] [--rate N/D] [--trim-audio] [--audio-shift N] [--codec-root PATH] [--tnv3-tool PATH] [--json]";

fn fail(code: u8, message: &str, json: bool) -> ExitCode {
    if json {
        let _ = write_stdout_line(
            &serde_json::json!({"command":"mux","schema":1,"status":"error","code":code,"message":message}).to_string(),
        );
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(code)
}

pub fn run(argv: &[OsString]) -> ExitCode {
    let json = argv.iter().any(|a| a == OsStr::new("--json"));
    let mut positional = Vec::new();
    let mut inputs = Inputs::default();
    let (mut root, mut tnv3) = (None, None);
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        let key = a.to_str().unwrap_or("");
        match key {
            "--json" => {}
            "--crc" => inputs.crc = true,
            "--trim-audio" => inputs.trim_audio = true,
            "--audio" | "--subtitles" | "--level" | "--rate" | "--audio-shift" | "--codec-root" | "--tnv3-tool" => {
                let Some(v) = argv.get(i + 1) else { return fail(2, USAGE, json) };
                i += 1;
                match key {
                    "--audio" => inputs.audio = Some(PathBuf::from(v)),
                    "--subtitles" => inputs.subtitles = Some(PathBuf::from(v)),
                    "--codec-root" => root = Some(PathBuf::from(v)),
                    "--tnv3-tool" => tnv3 = Some(PathBuf::from(v)),
                    "--level" => match v.to_str().and_then(|s| s.parse::<u8>().ok()) {
                        Some(l @ 1..=3) => inputs.level = Some(l),
                        _ => return fail(2, USAGE, json),
                    },
                    "--audio-shift" => match v.to_str().and_then(|s| s.parse::<i32>().ok()) {
                        Some(s) => inputs.audio_shift = Some(s),
                        None => return fail(2, USAGE, json),
                    },
                    _ => {
                        let parsed = v.to_str().and_then(|s| s.split_once('/')).and_then(|(n, d)| {
                            Some((n.parse::<u32>().ok()?, d.parse::<u32>().ok()?))
                        });
                        match parsed {
                            Some((n, d)) if n > 0 && d > 0 => inputs.rate = Some((n, d)),
                            _ => return fail(2, USAGE, json),
                        }
                    }
                }
            }
            _ if key.starts_with("--") => return fail(2, USAGE, json),
            _ => positional.push(PathBuf::from(a)),
        }
        i += 1;
    }
    let [video, output] = positional.as_slice() else { return fail(2, USAGE, json) };
    inputs.video = video.clone();
    if tnv3.is_none() && root.is_none() {
        return native(&inputs, output, json);
    }
    let explicit_root = root.is_some();
    let root = root.or_else(discover_codec_root);
    let tnv3 = tnv3
        .map(resolve_executable_arg)
        .or_else(|| select_codec_tool(root.as_deref(), explicit_root, "tnv3", "tools/tnv3_ref/tnv3"));
    let Some(tnv3) = tnv3 else {
        return fail(3, "Could not find the tnv3 tool; use --codec-root or --tnv3-tool.", json);
    };
    match c_mux::mux(&tnv3, &inputs, output) {
        Ok(r) => {
            let line = if json {
                serde_json::json!({"command":"mux","schema":1,"status":"ok","bytes":r.bytes,
                    "audio":r.audio,"subtitles":r.subtitles,"output":output})
                .to_string()
            } else {
                format!("Wrote {}: {} bytes, audio {}, subtitles {}", output.display(), r.bytes,
                    if r.audio {"yes"} else {"no"}, if r.subtitles {"yes"} else {"no"})
            };
            match write_stdout_line(&line) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(3, &format!("Could not write output: {e}"), false),
            }
        }
        Err(e) => {
            let code = match &e {
                MuxError::InvalidInput(_) => 2,
                MuxError::Spawn { .. } | MuxError::Io { .. } => 3,
                _ => 1,
            };
            fail(code, &e.to_string(), json)
        }
    }
}

/// The default: the pure-Rust muxer (`tau_video::container::mux_streams`), no C tool involved.
fn native(inputs: &Inputs, output: &Path, json: bool) -> ExitCode {
    use tau_video::container::{MuxOptions, mux_streams};
    let read = |p: &Path, what: &str| fs::read(p).map_err(|e| (if e.kind() == std::io::ErrorKind::NotFound { 2u8 } else { 3 }, format!("The {what} input {}: {e}", p.display())));
    let loaded = (|| {
        let video = read(&inputs.video, "video")?;
        let audio = inputs.audio.as_deref().map(|p| read(p, "audio")).transpose()?;
        let subs = inputs.subtitles.as_deref().map(|p| read(p, "subtitle area")).transpose()?;
        Ok::<_, (u8, String)>((video, audio, subs))
    })();
    let (video, audio, subs) = match loaded {
        Ok(v) => v,
        Err((c, m)) => return fail(c, &m, json),
    };
    let options = MuxOptions {
        audio: audio.as_deref(),
        subtitles: subs.as_deref(),
        crc: inputs.crc,
        level: inputs.level,
        rate: inputs.rate,
        trim_audio: inputs.trim_audio,
        audio_shift: inputs.audio_shift.unwrap_or(0),
    };
    let mut buffer = std::io::Cursor::new(Vec::new());
    let summary = match mux_streams(&video, &options, &mut buffer) {
        Ok(s) => s,
        Err(tau_video::container::MuxError::Io(e)) => return fail(3, &format!("Muxing failed: {e}"), json),
        Err(e) => return fail(1, &format!("Muxing failed: {e}"), json),
    };
    let bytes = buffer.into_inner();
    let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(output) {
        Ok(f) => f,
        Err(e) => return fail(if e.kind() == std::io::ErrorKind::AlreadyExists { 2 } else { 3 }, &format!("Output {}: {e}", output.display()), json),
    };
    use std::io::Write;
    if let Err(e) = f.write_all(&bytes).and_then(|_| f.sync_all()) {
        drop(f);
        let _ = fs::remove_file(output);
        return fail(3, &format!("Could not write the output: {e}"), json);
    }
    let line = if json {
        serde_json::json!({"command":"mux","schema":1,"status":"ok","bytes":bytes.len(),"audio":inputs.audio.is_some(),
            "subtitles":inputs.subtitles.is_some(),"frames":summary.frames,"level":summary.level,
            "audio_frames":summary.audio_frames,"audio_samples":summary.audio_samples,"muxer":"rust","output":output}).to_string()
    } else {
        format!("Wrote {}: {} bytes, {} pictures, level {}, audio {}, subtitles {} (Rust muxer)", output.display(), bytes.len(), summary.frames, summary.level,
            if inputs.audio.is_some() { "yes" } else { "no" }, if inputs.subtitles.is_some() { "yes" } else { "no" })
    };
    match write_stdout_line(&line) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(3, &format!("Could not write output: {e}"), false),
    }
}

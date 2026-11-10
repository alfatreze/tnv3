//! `encode-audio INPUT OUTPUT.tac`: encodes the first audio stream of any media file to TAC through
//! FFmpeg and the C reference encoder (see `tau_video::c_audio`). The result can be passed to `mux --audio`.
use super::*;
use tau_video::c_audio::{self, DecodeError, EncodeTools};

const USAGE: &str = "Usage: tau-codecs-cli encode-audio INPUT OUTPUT.tac [--bitrate 16..512] [--channels 1|2] [--track N] [--codec-root PATH] [--tac-tool PATH] [--ffmpeg PATH|NAME] [--ffprobe PATH|NAME] [--no-downmix-enhance] [--audio-loudness -40..-5] [--audio-floor -30..12] [--audio-tuned] [--json]";

fn fail(code: u8, message: &str, json: bool) -> ExitCode {
    if json {
        let _ = write_stdout_line(
            &serde_json::json!({"command":"encode-audio","schema":1,"status":"error","code":code,"message":message}).to_string(),
        );
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(code)
}

pub fn run(argv: &[OsString]) -> ExitCode {
    let json = argv.iter().any(|a| a == OsStr::new("--json"));
    let mut positional = Vec::new();
    let (mut kbps, mut channels) = (96u32, 2u8);
    let mut track = 0u32;
    let (mut root, mut tac, mut ffmpeg, mut ffprobe) = (None, None, None, None);
    let mut options = c_audio::EncodeOptions::default();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        let key = a.to_str().unwrap_or("");
        match key {
            "--json" => {}
            "--no-downmix-enhance" => options.prep.downmix_enhance = false,
            "--audio-tuned" => options.tuning.tuned = true,
            "--audio-loudness" | "--audio-floor" => {
                let Some(n) = argv.get(i + 1).and_then(|v| v.to_str()).and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite()) else { return fail(2, USAGE, json) };
                i += 1;
                if key == "--audio-loudness" && (-40.0..=-5.0).contains(&n) {
                    options.prep.loudness_lufs = n;
                } else if key == "--audio-floor" && (-30.0..=12.0).contains(&n) {
                    options.tuning.floor_db = Some(n);
                } else {
                    return fail(2, USAGE, json);
                }
            }
            "--bitrate" | "--channels" | "--track" | "--codec-root" | "--tac-tool" | "--ffmpeg" | "--ffprobe" => {
                let Some(v) = argv.get(i + 1) else { return fail(2, USAGE, json) };
                i += 1;
                match key {
                    "--bitrate" => match v.to_str().and_then(|s| s.parse::<u32>().ok()) {
                        Some(k @ 16..=512) => kbps = k,
                        _ => return fail(2, USAGE, json),
                    },
                    "--channels" => match v.to_str() {
                        Some("1") => channels = 1,
                        Some("2") => channels = 2,
                        _ => return fail(2, USAGE, json),
                    },
                    "--track" => match v.to_str().and_then(|s| s.parse::<u32>().ok()) {
                        Some(t) if t < 128 => track = t,
                        _ => return fail(2, USAGE, json),
                    },
                    "--codec-root" => root = Some(PathBuf::from(v)),
                    "--tac-tool" => tac = Some(PathBuf::from(v)),
                    "--ffprobe" => ffprobe = Some(PathBuf::from(v)),
                    _ => ffmpeg = Some(PathBuf::from(v)),
                }
            }
            _ if key.starts_with("--") => return fail(2, USAGE, json),
            _ => positional.push(PathBuf::from(a)),
        }
        i += 1;
    }
    let [input, output] = positional.as_slice() else { return fail(2, USAGE, json) };
    let explicit_root = root.is_some();
    let root = root.or_else(discover_codec_root);
    let tac = tac
        .map(resolve_executable_arg)
        .or_else(|| select_codec_tool(root.as_deref(), explicit_root, "tac", "tools/tac_ref/tac"));
    let ffmpeg = ffmpeg
        .map(resolve_executable_arg)
        .or_else(|| adjacent_executable("ffmpeg"))
        .or_else(|| on_path("ffmpeg"));
    options.ffprobe = ffprobe
        .map(resolve_executable_arg)
        .or_else(|| ffmpeg.as_ref().map(|f| f.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" })).filter(|p| p.is_file()))
        .or_else(|| adjacent_executable("ffprobe"))
        .or_else(|| on_path("ffprobe"));
    let (Some(tac), Some(ffmpeg)) = (tac, ffmpeg) else {
        return fail(3, "Could not find the tac tool and FFmpeg; use --codec-root, --tac-tool and --ffmpeg.", json);
    };
    match c_audio::encode_audio_with(&EncodeTools { ffmpeg, tac }, input, output, kbps, channels, None, track, &options) {
        Ok(r) => {
            let line = if json {
                serde_json::json!({"command":"encode-audio","schema":1,"status":"ok","bitrate_kbps":kbps,"audio_track":track,
                    "channels":r.channels,"samples_per_channel":r.samples_per_channel,"tac_frames":r.tac_frames,
                    "bytes":r.bytes,"output":output})
                .to_string()
            } else {
                format!("Wrote {}: {} channel(s), {} samples per channel, {} bytes", output.display(), r.channels, r.samples_per_channel, r.bytes)
            };
            match write_stdout_line(&line) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(3, &format!("Could not write output: {e}"), false),
            }
        }
        Err(e) => {
            let code = match &e {
                DecodeError::InvalidInput(_) => 2,
                DecodeError::Spawn { .. } | DecodeError::Io { .. } => 3,
                _ => 1,
            };
            fail(code, &e.to_string(), json)
        }
    }
}

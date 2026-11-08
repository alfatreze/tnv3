//! `decode-audio INPUT.tnv3 OUTPUT.wav`: decodes the TAC audio through the C reference tools
//! (see `tau_video::c_audio`). Not an independent decoder; no cancellation.
use super::*;
use tau_video::c_audio::{self, DecodeError, Model};

const USAGE: &str = "Usage: tau-codecs-cli decode-audio INPUT.tnv3 OUTPUT.wav [--model integer|reference] [--codec-root PATH] [--tnv3-tool PATH] [--tac-tool PATH] [--json]";

fn fail(code: u8, message: &str, json: bool) -> ExitCode {
    if json {
        let _ = write_stdout_line(
            &serde_json::json!({"command":"decode-audio","schema":1,"status":"error","code":code,"message":message}).to_string(),
        );
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(code)
}

pub fn run(argv: &[OsString]) -> ExitCode {
    let json = argv.iter().any(|a| a == OsStr::new("--json"));
    let mut positional = Vec::new();
    let (mut model, mut root, mut tnv3, mut tac) = (Model::Integer, None, None, None);
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        let key = a.to_str().unwrap_or("");
        match key {
            "--json" => {}
            "--model" | "--codec-root" | "--tnv3-tool" | "--tac-tool" => {
                let Some(v) = argv.get(i + 1) else { return fail(2, USAGE, json) };
                i += 1;
                match key {
                    "--model" => match v.to_str() {
                        Some("integer") => model = Model::Integer,
                        Some("reference") => model = Model::Reference,
                        _ => return fail(2, USAGE, json),
                    },
                    "--codec-root" => root = Some(PathBuf::from(v)),
                    "--tnv3-tool" => tnv3 = Some(PathBuf::from(v)),
                    _ => tac = Some(PathBuf::from(v)),
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
    let tac_name = if model == Model::Integer { "tac_fx" } else { "tac" };
    let tac_rel = if model == Model::Integer { "tools/tac_ref/tac_fx" } else { "tools/tac_ref/tac" };
    let tnv3 = tnv3
        .map(resolve_executable_arg)
        .or_else(|| select_codec_tool(root.as_deref(), explicit_root, "tnv3", "tools/tnv3_ref/tnv3"));
    let tac = tac
        .map(resolve_executable_arg)
        .or_else(|| select_codec_tool(root.as_deref(), explicit_root, tac_name, tac_rel));
    let (Some(tnv3), Some(tac)) = (tnv3, tac) else {
        return fail(3, &format!("Could not find the tnv3 and {tac_name} tools; use --codec-root, --tnv3-tool and --tac-tool."), json);
    };
    match c_audio::decode_audio(&c_audio::Tools { tnv3, tac }, input, output) {
        Ok(r) => {
            let line = if json {
                serde_json::json!({"command":"decode-audio","schema":1,"status":"ok",
                    "model": if model == Model::Integer {"integer"} else {"reference"},
                    "channels": r.channels, "samples_per_channel": r.samples_per_channel,
                    "wav_bytes": r.wav_bytes, "independent_decoder": false, "output": output})
                .to_string()
            } else {
                format!("Wrote {}: {} channel(s), {} samples per channel", output.display(), r.channels, r.samples_per_channel)
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

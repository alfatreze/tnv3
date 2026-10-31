//! Temporary MP3 sidecar adapter. The codec itself is provided by FFmpeg.
use std::{
    env,
    ffi::OsString,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let mode = args.next();
    let input = args.next();
    let output = args.next();
    if args.next().is_some()
        || input.is_none()
        || output.is_none()
        || !matches!(
            mode.as_deref().and_then(|s| s.to_str()),
            Some("encode" | "decode")
        )
    {
        eprintln!(
            "Usage: tau-mp3 encode INPUT.wav OUTPUT.mp3 | tau-mp3 decode INPUT.mp3 OUTPUT.wav"
        );
        return ExitCode::from(2);
    }
    let Some(input) = input else {
        return ExitCode::from(2);
    };
    let Some(output) = output else {
        return ExitCode::from(2);
    };
    let mut cmd =
        Command::new(env::var_os("TAU_FFMPEG").unwrap_or_else(|| OsString::from("ffmpeg")));
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-n", "-i"])
        .arg(input);
    if mode.as_deref() == Some(std::ffi::OsStr::new("encode")) {
        cmd.args([
            "-vn",
            "-map_metadata",
            "-1",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
            "-f",
            "mp3",
        ]);
    } else {
        cmd.args([
            "-vn",
            "-map_metadata",
            "-1",
            "-ar",
            "48000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ]);
    }
    match cmd.arg(output).status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("FFmpeg failed ({s}).");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Could not start FFmpeg: {e}. Set TAU_FFMPEG to its path if needed.");
            ExitCode::from(3)
        }
    }
}

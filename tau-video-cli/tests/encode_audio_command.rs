//! Wiring and error-path tests for `encode-audio` with fake tools (Unix only).
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}
// fake ffmpeg: records args, writes the last argument; fake tac: writes a 20-byte TAC0 header ($3 = output)
const FF: &str = "echo \"$@\" > ff.txt; for a; do last=$a; done; echo pcm > \"$last\"";
fn tac(ch: u8) -> String {
    format!("echo \"$@\" > tac.txt; printf 'TAC0\\200\\273\\000\\000\\00{ch}\\000\\000\\000\\003\\000\\000\\000\\000\\004\\000\\000' > \"$3\"")
}
fn run(dir: &Path, ff: &Path, tc: &Path, extra: &[&str], out: &str) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["encode-audio", "in.mkv", out, "--json", "--ffmpeg"])
        .arg(ff)
        .arg("--tac-tool")
        .arg(tc)
        .args(extra)
        .current_dir(dir)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).into_owned())
}
fn setup(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tau-encaudio-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("in.mkv"), b"x").unwrap();
    dir
}

#[test]
fn runs_ffmpeg_then_encoder_with_options_and_never_overwrites() {
    let dir = setup("ok");
    let (ff, tc) = (script(&dir, "ffmpeg", FF), script(&dir, "tac", &tac(1)));
    let (code, out) = run(&dir, &ff, &tc, &["--bitrate", "128", "--channels", "1"], "o.tac");
    assert_eq!(code, 0, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((j["channels"].as_u64(), j["tac_frames"].as_u64(), j["samples_per_channel"].as_u64()), (Some(1), Some(3), Some(1024)));
    let f = fs::read_to_string(dir.join("ff.txt")).unwrap();
    assert!(f.contains("-map 0:a:0") && f.contains("-ar 48000 -ac 1"), "{f}");
    assert!(fs::read_to_string(dir.join("tac.txt")).unwrap().trim_end().ends_with("-b 128"));
    assert_eq!(run(&dir, &ff, &tc, &[], "o.tac").0, 2);
    assert!(fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".tau-audio")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reports_bad_options_failures_and_channel_mismatch_without_output() {
    let dir = setup("err");
    let (ff, tc) = (script(&dir, "ffmpeg", FF), script(&dir, "tac", &tac(2)));
    assert_eq!(run(&dir, &ff, &tc, &["--bitrate", "5"], "o.tac").0, 2);
    assert_eq!(run(&dir, &ff, &tc, &["--channels", "6"], "o.tac").0, 2);
    // encoder header says 2 channels but 1 was requested -> tool failure
    assert_eq!(run(&dir, &ff, &tc, &["--channels", "1"], "o.tac").0, 1);
    let bad_ff = script(&dir, "badff", "echo 'no audio' >&2; exit 1");
    let (code, out) = run(&dir, &bad_ff, &tc, &[], "o.tac");
    assert_eq!(code, 1);
    assert!(out.contains("no audio"), "{out}");
    assert_eq!(run(&dir, &dir.join("nope"), &tc, &[], "o.tac").0, 3);
    assert!(!dir.join("o.tac").exists());
    fs::remove_dir_all(&dir).unwrap();
}

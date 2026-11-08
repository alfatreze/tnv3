//! Wiring and error-path tests for `decode-audio` with fake tool scripts (Unix only).
//! The real C tools are exercised by the manual end-to-end run recorded in docs, not here.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}
fn wav(channels: u8, frames: u32) -> String {
    // printf-able 44-byte header + `frames` stereo/mono 16-bit zero samples
    let data = frames * 2 * u32::from(channels);
    let le = |v: u32| format!("\\{:03o}\\{:03o}\\{:03o}\\{:03o}", v & 255, v >> 8 & 255, v >> 16 & 255, v >> 24);
    format!(
        "printf 'RIFF{}WAVEfmt {}\\001\\000\\00{}\\000\\200\\273\\000\\000\\000\\356\\002\\000\\004\\000\\020\\000data{}' > \"$3\"; head -c {data} /dev/zero >> \"$3\"",
        le(36 + data), le(16), channels, le(data)
    )
}
fn run(dir: &Path, tnv3: &Path, tac: &Path, out: &str) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["decode-audio", "in.tnv3", out, "--json", "--tnv3-tool"])
        .arg(tnv3)
        .arg("--tac-tool")
        .arg(tac)
        .current_dir(dir)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).into_owned())
}

#[test]
fn wires_demux_then_decoder_and_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("tau-decode-audio-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("in.tnv3"), b"x").unwrap();
    // fake demux writes the .tac (argv: demux IN VIDEO AUDIO); fake decoder writes a WAV
    let demux = script(&dir, "tnv3", "echo tac > \"$4\"");
    let dec = script(&dir, "tac", &wav(2, 10));
    let (code, out) = run(&dir, &demux, &dec, "out.wav");
    assert_eq!(code, 0, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((j["channels"].as_u64(), j["samples_per_channel"].as_u64()), (Some(2), Some(10)));
    assert_eq!(j["independent_decoder"], false);
    assert_eq!(fs::metadata(dir.join("out.wav")).unwrap().len(), 44 + 40);
    // second run: output exists -> usage-class error 2, file untouched, no workspace left
    let (code, _) = run(&dir, &demux, &dec, "out.wav");
    assert_eq!(code, 2);
    assert!(fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".tau-audio")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reports_no_audio_tool_failure_and_bad_wav_without_leaving_output() {
    let dir = std::env::temp_dir().join(format!("tau-decode-audio-err-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("in.tnv3"), b"x").unwrap();
    let good_dec = script(&dir, "tac", &wav(1, 4));
    let no_audio = script(&dir, "tnv3_noaudio", "exit 0");
    let (code, out) = run(&dir, &no_audio, &good_dec, "a.wav");
    assert_eq!(code, 1);
    assert!(out.contains("no audio stream"), "{out}");
    let bad_demux = script(&dir, "tnv3_bad", "echo invalid >&2; exit 3");
    let (code, out) = run(&dir, &bad_demux, &good_dec, "b.wav");
    assert_eq!(code, 1);
    assert!(out.contains("tnv3 demux failed"), "{out}");
    let demux = script(&dir, "tnv3", "echo tac > \"$4\"");
    let junk_dec = script(&dir, "junk", "echo notawav > \"$3\"");
    let (code, out) = run(&dir, &demux, &junk_dec, "c.wav");
    assert_eq!(code, 1);
    assert!(out.contains("no valid WAV header"), "{out}");
    for n in ["a.wav", "b.wav", "c.wav"] {
        assert!(!dir.join(n).exists(), "{n}");
    }
    let missing = dir.join("does-not-exist");
    let (code, _) = run(&dir, &missing, &good_dec, "d.wav");
    assert_eq!(code, 3);
    fs::remove_dir_all(&dir).unwrap();
}

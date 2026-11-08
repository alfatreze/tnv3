//! Wiring and error-path tests for `build-subtitles` with a fake Python (Unix only). The real builder
//! is exercised by the manual end-to-end run recorded in the docs.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}
fn setup(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tau-subs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    for f in ["a.srt", "builder.py", "font.ttf"] {
        fs::write(dir.join(f), b"x").unwrap();
    }
    dir
}
// fake python: $1 = builder, last argument = output area
const OK: &str = "echo \"$@\" > args.txt; for a; do last=$a; done; head -c 100 /dev/zero > \"$last\"; echo 'a.srt 20x28: 1 events'; echo 'warning: event at 15320 ms does not fit above the stack; clamped' >&2";
fn run(dir: &Path, python: &Path, extra: &[&str], out: &str) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["build-subtitles", out, "--video", "400x224", "--builder"])
        .arg(dir.join("builder.py"))
        .arg("--python")
        .arg(python)
        .args(extra)
        .arg("--json")
        .current_dir(dir)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).into_owned())
}

#[test]
fn builds_reports_warnings_and_never_overwrites() {
    let dir = setup("ok");
    let py = script(&dir, "py", OK);
    let (code, out) = run(&dir, &py, &["--track", "a.srt:eng:default", "--font", "font.ttf", "--sizes", "16x24", "--fps", "24/1"], "o.bin");
    assert_eq!(code, 0, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["bytes"], 100);
    assert!(j["warnings"][0].as_str().unwrap().contains("clamped"), "builder warnings are reported: {out}");
    let args = fs::read_to_string(dir.join("args.txt")).unwrap();
    assert!(args.contains("--track a.srt:eng:default") && args.contains("--font font.ttf") && args.contains("--video 400x224") && args.contains("--fps 24/1"), "{args}");
    assert_eq!(run(&dir, &py, &["--track", "a.srt", "--font", "font.ttf"], "o.bin").0, 2);
    assert!(fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".tau-subs")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rejects_bad_usage_missing_inputs_and_builder_failure_without_output() {
    let dir = setup("err");
    let py = script(&dir, "py", "echo 'tst_build: font file not found' >&2; exit 2");
    assert_eq!(run(&dir, &py, &[], "o.bin").0, 2, "no track");
    assert_eq!(run(&dir, &py, &["--track", "a.srt:eng:loud"], "o.bin").0, 2);
    assert_eq!(run(&dir, &py, &["--source-track", "0"], "o.bin").0, 2, "needs --from-source");
    assert_eq!(run(&dir, &py, &["--track", "missing.srt", "--font", "font.ttf"], "o.bin").0, 2);
    let (code, out) = run(&dir, &py, &["--track", "a.srt", "--font", "font.ttf"], "o.bin");
    assert_eq!(code, 1);
    assert!(out.contains("font file not found"), "{out}");
    assert!(!dir.join("o.bin").exists());
    fs::remove_dir_all(&dir).unwrap();
}

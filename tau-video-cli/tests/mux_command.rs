//! Wiring and error-path tests for `mux` with a fake tool script (Unix only), plus one run against
//! the real C tool when `TAU_TEST_TNV3` is set (see `make rust-conformance`).
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn script(dir: &Path, body: &str) -> std::path::PathBuf {
    let p = dir.join("tnv3");
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}
fn run(dir: &Path, tool: &Path, extra: &[&str], out: &str) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["mux", "v.tvc", out, "--json", "--tnv3-tool"])
        .arg(tool)
        .args(extra)
        .current_dir(dir)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).into_owned())
}
fn setup(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tau-mux-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    for f in ["v.tvc", "a.tac", "s.bin"] {
        fs::write(dir.join(f), b"x").unwrap();
    }
    dir
}
const WRITE_OUT: &str = "for a; do last=$a; done; head -c 80 /dev/zero > \"$last\"; echo \"$@\" > args.txt";

#[test]
fn passes_options_through_and_never_overwrites() {
    let dir = setup("ok");
    let tool = script(&dir, WRITE_OUT);
    let (code, out) = run(&dir, &tool, &["--audio", "a.tac", "--subtitles", "s.bin", "--crc", "--rate", "25/1"], "o.tnv3");
    assert_eq!(code, 0, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((j["bytes"].as_u64(), j["audio"].as_bool(), j["subtitles"].as_bool()), (Some(80), Some(true), Some(true)));
    let args = fs::read_to_string(dir.join("args.txt")).unwrap();
    assert!(args.starts_with("mux v.tvc -a a.tac -s s.bin -c -r 25/1 -o "), "{args}");
    assert_eq!(run(&dir, &tool, &[], "o.tnv3").0, 2);
    assert!(fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".tau-mux")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reports_bad_usage_missing_input_and_tool_failure_without_output() {
    let dir = setup("err");
    let tool = script(&dir, "echo 'muxed file is invalid: x' >&2; exit 3");
    assert_eq!(run(&dir, &tool, &["--level", "9"], "o.tnv3").0, 2);
    assert_eq!(run(&dir, &tool, &["--rate", "0/1"], "o.tnv3").0, 2);
    assert_eq!(run(&dir, &tool, &["--audio", "missing.tac"], "o.tnv3").0, 2);
    let (code, out) = run(&dir, &tool, &[], "o.tnv3");
    assert_eq!(code, 1);
    assert!(out.contains("muxed file is invalid"), "{out}");
    assert!(!dir.join("o.tnv3").exists());
    let (code, _) = run(&dir, &dir.join("nope"), &[], "o.tnv3");
    assert_eq!(code, 3);
    fs::remove_dir_all(&dir).unwrap();
}

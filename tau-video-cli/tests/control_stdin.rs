#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

fn shell_quote(value: &std::path::Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
}

fn temp_dir() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tau-cli-control-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn executable(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn control_eof_stops_direct_ffmpeg_leaf_without_publishing_or_overwriting() {
    let dir = temp_dir();
    let input = dir.join("input.mov");
    let ffmpeg = dir.join("ffmpeg");
    let tvc = dir.join("tvc_enc");
    let tnv3 = dir.join("tnv3");
    let marker = dir.join("started");
    let output = dir.join("output.tnv3");
    fs::write(&input, b"fixture").unwrap();
    fs::write(&tvc, b"unused").unwrap();
    fs::write(&tnv3, b"unused").unwrap();
    executable(
        &ffmpeg,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nprintf 'YUV4MPEG2 W400 H360 F24:1 Ip A1:1 C420jpeg\\n'\nwhile :; do printf 'diagnostic flood diagnostic flood diagnostic flood\\n' >&2; done\n",
            marker.display()
        ),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args([
            "encode",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--video-only",
            "--rust-c-orchestrator",
            "--control-stdin",
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--tvc-encoder",
            tvc.to_str().unwrap(),
            "--tnv3-tool",
            tnv3.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "fake FFmpeg did not start");

    // Simulate a writer arriving during the encode; no-replace publication and
    // the cancellation gate must preserve it byte-for-byte.
    fs::write(&output, b"concurrent writer wins").unwrap();
    drop(child.stdin.take()); // EOF is the sole cancellation signal.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(4));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "CLI did not stop after control EOF"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(&output).unwrap(), b"concurrent writer wins");

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sigint_stops_direct_ffmpeg_leaf_without_publishing() {
    let dir = temp_dir();
    let input = dir.join("input.mov");
    let ffmpeg = dir.join("ffmpeg");
    let tvc = dir.join("tvc_enc");
    let tnv3 = dir.join("tnv3");
    let marker = dir.join("started");
    let output = dir.join("output.tnv3");
    fs::write(&input, b"fixture").unwrap();
    fs::write(&tvc, b"unused").unwrap();
    fs::write(&tnv3, b"unused").unwrap();
    executable(
        &ffmpeg,
        &format!(
            "#!/usr/bin/env python3\nimport pathlib, time\npathlib.Path('{}').write_text('started')\nwhile True: time.sleep(1)\n",
            marker.display()
        ),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args([
            "encode",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--video-only",
            "--rust-c-orchestrator",
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--tvc-encoder",
            tvc.to_str().unwrap(),
            "--tnv3-tool",
            tnv3.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "fake FFmpeg did not start");

    // Signal only the CLI. It must request child cleanup, await it, and return
    // the normal cancellation status instead of dying immediately.
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(4));
            break;
        }
        assert!(Instant::now() < deadline, "CLI did not stop after SIGINT");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!output.exists(), "cancelled encode published output");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn terminal_process_group_sigint_is_handled_without_publishing() {
    let dir = temp_dir();
    let input = dir.join("input.mov");
    let ffmpeg = dir.join("ffmpeg");
    let launcher = dir.join("isolated-cli");
    let tvc = dir.join("tvc_enc");
    let tnv3 = dir.join("tnv3");
    let marker = dir.join("started");
    let output = dir.join("output.tnv3");
    fs::write(&input, b"fixture").unwrap();
    fs::write(&tvc, b"unused").unwrap();
    fs::write(&tnv3, b"unused").unwrap();
    executable(
        &ffmpeg,
        &format!(
            "#!/usr/bin/env python3\nimport pathlib, time\npathlib.Path('{}').write_text('started')\nwhile True: time.sleep(1)\n",
            marker.display()
        ),
    );
    // Python creates a dedicated session and execs the CLI in place. Sending
    // SIGINT to this process group models a terminal Ctrl-C without signaling
    // the test harness.
    executable(
        &launcher,
        &format!(
            "#!/usr/bin/env python3\nimport os, sys\nos.setsid()\nos.execv('{}', ['tau-codecs-cli', *sys.argv[1:]])\n",
            env!("CARGO_BIN_EXE_tau-codecs-cli")
        ),
    );
    let mut child = Command::new(&launcher)
        .args([
            "encode",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--video-only",
            "--rust-c-orchestrator",
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--tvc-encoder",
            tvc.to_str().unwrap(),
            "--tnv3-tool",
            tnv3.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "fake FFmpeg did not start");
    assert!(
        Command::new("kill")
            .args(["-INT", &format!("-{}", child.id())])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(4));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "CLI did not stop after process-group SIGINT"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!output.exists(), "cancelled encode published output");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sigint_during_audio_stops_before_publishing_pair() {
    let (Some(real_ffmpeg), Some(tvc), Some(tnv3)) = (
        std::env::var_os("TAU_TEST_FFMPEG"),
        std::env::var_os("TAU_TEST_TVC"),
        std::env::var_os("TAU_TEST_TNV3"),
    ) else {
        return;
    };
    let dir = temp_dir();
    let input = dir.join("input.mkv");
    assert!(
        Command::new(&real_ffmpeg)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=24:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
                "-shortest"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let ffmpeg = dir.join("ffmpeg");
    let marker = dir.join("audio-started");
    let real = std::path::PathBuf::from(&real_ffmpeg);
    executable(
        &ffmpeg,
        &format!(
            "#!/bin/sh\ncase \" $* \" in *' 0:a:0 '*) : > {}; while :; do :; done;; esac\nexec {} \"$@\"\n",
            shell_quote(&marker),
            shell_quote(&real)
        ),
    );
    let output = dir.join("output.tnv3");
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("encode")
        .arg(&input)
        .arg(&output)
        .args(["--rust-c-orchestrator", "--json-events", "--ffmpeg"])
        .arg(&ffmpeg)
        .arg("--tvc-encoder")
        .arg(tvc)
        .arg("--tnv3-tool")
        .arg(tnv3)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() && Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "CLI exited before audio started"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "audio did not start");
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(4));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "CLI did not stop after SIGINT during audio"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!output.exists());
    assert!(!output.with_extension("mp3").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn control_stdin_kept_open_does_not_block_success() {
    let (Some(ffmpeg), Some(tvc), Some(tnv3)) = (
        std::env::var_os("TAU_TEST_FFMPEG"),
        std::env::var_os("TAU_TEST_TVC"),
        std::env::var_os("TAU_TEST_TNV3"),
    ) else {
        return;
    };
    let dir = temp_dir();
    let input = dir.join("input.mkv");
    assert!(
        Command::new(&ffmpeg)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=24:duration=0.5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.5",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
                "-shortest"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let output = dir.join("success.tnv3");
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("encode")
        .arg(&input)
        .arg(&output)
        .args(["--rust-c-orchestrator", "--json-events", "--ffmpeg"])
        .arg(ffmpeg)
        .arg("--tvc-encoder")
        .arg(tvc)
        .arg("--tnv3-tool")
        .arg(tnv3)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "open control pipe blocked successful completion"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(output.is_file());
    assert!(output.with_extension("mp3").is_file());
    fs::remove_dir_all(dir).unwrap();
}

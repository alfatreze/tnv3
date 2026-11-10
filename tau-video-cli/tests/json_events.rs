use std::process::Command;
#[cfg(unix)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
fn temp_dir() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("tau-json-cli-{}-{id}", std::process::id()));
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
fn executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
}

#[test]
fn usage_error_is_one_json_line_and_keeps_stdout_machine_readable() {
    let output = cli()
        .args([
            "encode",
            "input.mp4",
            "output.tnv3",
            "--python-c", "--json-events",
            "--unknown",
            "x",
        ])
        .output()
        .expect("CLI process should start");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("JSON output is UTF-8");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let event: serde_json::Value = serde_json::from_str(lines[0]).expect("one valid JSON event");
    assert_eq!(event["schema"], 1);
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], 2);
    assert!(
        event["message"]
            .as_str()
            .unwrap()
            .contains("unknown option")
    );
}

#[test]
fn version_json_reports_cli_and_format_without_guessing_tool_versions() {
    let output = cli()
        .args(["--version", "--json"])
        .output()
        .expect("CLI process should start");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let report: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(
        report,
        serde_json::json!({
            "schema": 1,
            "cli": {"name": "tau-codecs-cli", "version": env!("CARGO_PKG_VERSION")},
            "formats": {"tnv3_container": 0},
            "tools": {"tvc_enc": null, "tnv3": null, "ffmpeg": null, "ffprobe": null},
            "tools_status": "not_probed"
        })
    );
}

#[test]
fn handshake_reports_scoped_format_support_without_running_tools() {
    let output = cli().args(["handshake", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], "handshake");
    assert_eq!(report["formats"]["tnv3_container"]["version"], 0);
    assert_eq!(
        report["formats"]["tvc_elementary"]["read_magics"][0],
        "TVC0"
    );
    assert_eq!(
        report["formats"]["tvc_elementary"]["write_magics"][0],
        "TVC1"
    );
    assert_eq!(report["formats"]["tac_elementary"]["read"], false);
    assert_eq!(report["tools"], serde_json::json!({}));
    assert_eq!(report["tools_status"], "not_selected");
    // With no explicit paths, the handshake does not inspect PATH or ask any
    // installed tool for a version; it reports only its own compiled facts.
    assert!(report["tools"].as_object().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn handshake_hashes_only_explicit_tool_paths_and_keeps_version_unknown() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let tool = dir.join("ffmpeg");
    // This executable would leave evidence if the handshake queried it.
    let marker = dir.join("was-executed");
    let probe = dir.join("probe-tool");
    executable(
        &probe,
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    // The known payload makes this an end-to-end SHA-256 correctness check,
    // rather than merely checking that a digest has the expected shape.
    std::fs::write(&tool, b"abc").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = cli()
        .args(["handshake", "--json", "--ffmpeg"])
        .arg(&tool)
        .arg("--ffprobe")
        .arg(&probe)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["tools"]["ffmpeg"]["version"],
        serde_json::Value::Null
    );
    assert_eq!(report["tools"]["ffmpeg"]["version_status"], "not_exposed");
    assert_eq!(
        report["tools"]["ffmpeg"]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        report["tools"]["ffprobe"]["version"],
        serde_json::Value::Null
    );
    assert_eq!(report["tools"]["ffprobe"]["version_status"], "not_exposed");
    assert!(
        !marker.exists(),
        "handshake must hash selected files without querying their version"
    );
    assert_eq!(
        report["tools_status"],
        "explicit_files_hashed_versions_not_exposed"
    );
    assert_eq!(
        report["tools"]["ffmpeg"]["sha256"].as_str().unwrap().len(),
        64
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn handshake_reports_malformed_arguments_and_explicit_file_errors() {
    for (args, code, expected) in [
        (vec!["handshake", "--ffmpeg", "tool"], 2, "requires --json"),
        (
            vec!["handshake", "--json", "--unknown", "tool"],
            2,
            "unknown tool-identity option",
        ),
        (vec!["handshake", "--json", "--ffmpeg"], 2, "missing path"),
    ] {
        let output = cli().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["command"], "handshake");
        assert_eq!(report["error"]["code"], code);
        assert!(
            report["error"]["message"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
    }

    #[cfg(unix)]
    {
        let dir = temp_dir();
        let absent = dir.join("missing-tool");
        let output = cli()
            .args(["handshake", "--json", "--ffmpeg"])
            .arg(&absent)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"]["code"], 3);
        assert!(
            report["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Could not safely open")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn handshake_broken_pipe_returns_io_error_without_panicking() {
    // Closing the read side before the child starts makes EPIPE deterministic.
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let output = cli()
        .args(["handshake", "--json"])
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(writer)))
        .stderr(Stdio::piped())
        .output()
        .expect("CLI process should start");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Could not write handshake output"));
}

#[test]
fn plain_version_prints_cli_name_and_package_version() {
    let output = cli().arg("--version").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("tau-codecs-cli {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn version_flags_reject_other_orderings_and_extra_arguments() {
    for args in [
        vec!["--json", "--version"],
        vec!["--version", "extra"],
        vec!["encode", "input.mp4", "out.tnv3", "--version"],
    ] {
        let output = cli().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn version_json_broken_pipe_returns_error_without_panicking() {
    // Drop the read end before spawning, so the write cannot win a race.
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let output = cli()
        .args(["--version", "--json"])
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(writer)))
        .stderr(Stdio::piped())
        .output()
        .expect("CLI process should start");
    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Could not write version output"));
}

#[cfg(unix)]
fn fake_encode_setup() -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let python = dir.join("fake-python");
    let script = dir.join("encode.py");
    let ffmpeg = dir.join("ffmpeg-unused");
    let ffprobe = dir.join("ffprobe-unused");
    fs::write(&input, b"input").unwrap();
    fs::write(&script, b"placeholder").unwrap();
    executable(&ffmpeg, "#!/bin/sh\nexit 0\n");
    executable(&ffprobe, "#!/bin/sh\nexit 0\n");
    executable(
        &python,
        r##"#!/bin/sh
out="$3"
printf 'fixture-video' > "$out"
cat > "${out%.tnv3}.json" <<'REPORT'
{"valid":true,"frames":12,"level":2,"pictures_raised_for_level":7}
REPORT
if [ -n "$TAU_TEST_RELEASE" ]; then
  while [ ! -e "$TAU_TEST_RELEASE" ]; do sleep 0.01; done
fi
printf 'encoded 1 / 1\n'
if [ -n "$TAU_TEST_FINISHED" ]; then : > "$TAU_TEST_FINISHED"; fi
"##,
    );
    (dir, input, python, script, ffmpeg, ffprobe)
}

#[cfg(unix)]
fn encode_args(
    input: &Path,
    output: &Path,
    python: &Path,
    script: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
) -> Command {
    let mut command = cli();
    command
        .arg("encode")
        .arg(input)
        .arg(output)
        .args(["--python-c", "--json-events", "--video-only", "--python"])
        .arg(python)
        .arg("--script")
        .arg(script)
        .arg("--ffmpeg")
        .arg(ffmpeg)
        .arg("--ffprobe")
        .arg(ffprobe);
    command
}

#[cfg(unix)]
#[test]
fn encode_reports_hash_and_numeric_picture_retry_count() {
    use sha2::Digest;
    let (dir, input, python, script, ffmpeg, ffprobe) = fake_encode_setup();
    let output = dir.join("result.tnv3");
    let result = encode_args(&input, &output, &python, &script, &ffmpeg, &ffprobe)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(result.stderr.is_empty());
    let events: Vec<serde_json::Value> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.first().unwrap()["event"], "plan");
    let effective = &events[0]["effective_settings"];
    assert_eq!(
        effective["interpretation"],
        "requested-workflow-settings-not-measured-output"
    );
    assert_eq!(effective["external_tools_may_ignore_settings"], true);
    assert_eq!(effective["video"]["qp_may_be_raised_for_level"], true);
    assert_eq!(effective["video"]["max_width"], 400);
    assert_eq!(effective["video"]["fps_num"], 24);
    assert_eq!(effective["video"]["frames_per_segment"], 480);
    assert_eq!(effective["video"]["worker_jobs_limit"], 1);
    assert!(effective["audio"].is_null());
    assert_eq!(
        events.first().unwrap()["input"],
        input.to_string_lossy().as_ref()
    );
    assert_eq!(events.last().unwrap()["event"], "done");
    let progress = events
        .iter()
        .find(|event| event["event"] == "progress")
        .unwrap();
    assert!(progress["bytes"].is_null());
    let done = events.last().unwrap();
    assert_eq!(done["reencoded"], 7);
    assert_eq!(done["reencoded_kind"], "pictures_raised_for_level");
    assert_eq!(done["sizes"][0], b"fixture-video".len());
    assert_eq!(done["files"][0], output.to_string_lossy().as_ref());
    assert_eq!(
        done["sha256"][0],
        format!("{:x}", sha2::Sha256::digest(b"fixture-video"))
    );
    assert!(output.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn missing_input_is_typed_as_io_exit_three() {
    let dir = temp_dir();
    let fake = dir.join("tool");
    executable(&fake, "#!/bin/sh\nexit 0\n");
    let output = cli()
        .args([
            "encode",
            "missing.mp4",
            "out.tnv3",
            "--json-events",
            "--rust-c-orchestrator",
            "--video-only",
            "--ffmpeg",
        ])
        .arg(&fake)
        .arg("--tvc-encoder")
        .arg(&fake)
        .arg("--tnv3-tool")
        .arg(&fake)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let event: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], 3);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn non_executable_python_spawn_failure_is_typed_as_io_exit_three() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, input, python, script, ffmpeg, ffprobe) = fake_encode_setup();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o600)).unwrap();
    let output = dir.join("unused.tnv3");
    let result = encode_args(&input, &output, &python, &script, &ffmpeg, &ffprobe)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    let event: serde_json::Value = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .last()
        .unwrap();
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], 3);
    assert!(
        event["message"]
            .as_str()
            .unwrap()
            .contains("Could not start C encoder")
    );
    assert!(!output.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn missing_report_is_io_but_malformed_report_is_unclassified() {
    for (body, expected) in [
        ("#!/bin/sh\nexit 0\n", 3),
        ("#!/bin/sh\nprintf '{bad' > \"${3%.tnv3}.json\"\n", 1),
        (
            "#!/bin/sh\nprintf '{\"valid\":true,\"frames\":1,\"level\":1,\"pictures_raised_for_level\":0}' > \"${3%.tnv3}.json\"\n",
            3,
        ),
    ] {
        let (dir, input, python, script, ffmpeg, ffprobe) = fake_encode_setup();
        executable(&python, body);
        let output = dir.join("unused.tnv3");
        let result = encode_args(&input, &output, &python, &script, &ffmpeg, &ffprobe)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(expected));
        let event: serde_json::Value = String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .last()
            .unwrap();
        assert_eq!(event["event"], "error");
        assert_eq!(event["code"], expected);
        assert!(!output.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn closed_stdout_fails_before_starting_encoder_without_panicking() {
    let (dir, input, python, script, ffmpeg, ffprobe) = fake_encode_setup();
    let output_path = dir.join("never-written.tnv3");
    let mut child = encode_args(&input, &output_path, &python, &script, &ffmpeg, &ffprobe)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(3));
    assert!(!output_path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn broken_progress_stream_waits_for_encoder_completion_and_discards_staged_output() {
    use std::io::{BufRead, BufReader, Read};
    let (dir, input, python, script, ffmpeg, ffprobe) = fake_encode_setup();
    let release = dir.join("release-child");
    let finished = dir.join("encoder-finished");
    let output = dir.join("must-not-publish.tnv3");
    let mut child = encode_args(&input, &output, &python, &script, &ffmpeg, &ffprobe)
        .env("TAU_TEST_RELEASE", &release)
        .env("TAU_TEST_FINISHED", &finished)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut plan = String::new();
    stdout.read_line(&mut plan).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&plan).unwrap()["event"],
        "plan"
    );
    drop(stdout);
    fs::write(&release, b"go").unwrap();

    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(3), "{stderr}");
    assert!(
        finished.exists(),
        "fake encoder should finish before CLI returns"
    );
    assert!(
        !output.exists(),
        "staged output must not be published after progress write failure; status={status:?}; stderr={stderr:?}; finished={}; output_exists={}",
        finished.exists(),
        output.exists()
    );
    assert!(
        stderr.contains("JSON progress reporting failed"),
        "{stderr}"
    );
    fs::remove_dir_all(dir).unwrap();
}

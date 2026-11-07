use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
}

fn assert_json_error(
    output: Output,
    expected_status: i32,
    expected_code: u64,
    expected_kind: &str,
    message: &str,
) {
    assert_eq!(output.status.code(), Some(expected_status));
    assert!(
        output.stderr.is_empty(),
        "JSON mode should keep diagnostics in the event stream; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("JSON Lines output should be UTF-8");
    let lines: Vec<_> = stdout.lines().collect();
    assert!(!lines.is_empty(), "expected JSON event output");
    let events: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("valid JSON event"))
        .collect();
    let event = events.last().unwrap();
    assert_eq!(
        event["event"], "error",
        "last event should report failure: {stdout:?}"
    );
    assert!(
        events[..events.len() - 1].iter().all(|prior| matches!(
            prior["event"].as_str(),
            Some("plan" | "progress" | "warning")
        )),
        "unexpected event before error: {stdout:?}"
    );
    assert_eq!(event["schema"], 1);
    assert_eq!(event["event"], "error");
    assert_eq!(event["code"], expected_code);
    assert_eq!(
        event["kind"], expected_kind,
        "unexpected error kind: {event}"
    );
    assert!(
        event["message"]
            .as_str()
            .unwrap_or_default()
            .contains(message),
        "unexpected error message: {event}"
    );
}

#[test]
fn usage_error_has_stable_status_and_json_error_event() {
    let output = cli()
        .args([
            "encode",
            "input.mp4",
            "output.tnv3",
            "--json-events",
            "--unknown-option",
            "x",
        ])
        .output()
        .expect("CLI process should start");

    assert_json_error(output, 2, 2, "usage", "unknown option");
}

#[test]
fn invalid_argument_value_is_classified_as_usage() {
    let output = cli()
        .args([
            "encode",
            "input.mp4",
            "output.tnv3",
            "--json-events",
            "--qp",
            "not-a-number",
        ])
        .output()
        .expect("CLI process should start");

    assert_json_error(output, 2, 2, "usage", "QP must be an integer");
}

#[cfg(unix)]
#[test]
fn syntax_error_stays_usage_when_path_has_no_dependencies() {
    let dir = temp_dir();
    let output = cli()
        .args([
            "encode",
            "input.mp4",
            "output.tnv3",
            "--json-events",
            "--unknown-option",
            "value",
        ])
        .env("PATH", &dir)
        .env_remove("TAU_CODECS_ROOT")
        .current_dir(&dir)
        .output()
        .expect("CLI process should start");

    assert_json_error(output, 2, 2, "usage", "unknown option");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn missing_input_is_io_status_with_json_error_event() {
    let dir = temp_dir();
    let output_path = dir.join("out.tnv3");
    let result = cli()
        .args([
            "encode",
            dir.join("absent.mp4").to_str().unwrap(),
            output_path.to_str().unwrap(),
            "--json-events",
            "--video-only",
        ])
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 3, 3, "io", "Input");
    assert!(
        !output_path.exists(),
        "failed preflight must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn missing_encoder_dependency_is_classified_without_changing_exit_code() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    let missing_ffmpeg = dir.join("missing-ffmpeg");
    std::fs::write(&input, b"fixture input").unwrap();

    let result = cli()
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only", "--ffmpeg"])
        .arg(&missing_ffmpeg)
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 3, 3, "dependency", "FFmpeg does not exist");
    assert!(
        !output_path.exists(),
        "preflight failure must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn automatically_discovered_tools_absent_from_isolated_path_are_dependency_errors() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    std::fs::write(&input, b"fixture input").unwrap();

    let result = cli()
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only"])
        .env("PATH", &dir)
        .env_remove("TAU_CODECS_ROOT")
        .current_dir(&dir)
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 3, 3, "dependency", "not found");
    assert!(
        !output_path.exists(),
        "missing tool must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn automatically_missing_ffmpeg_is_dependency_error_after_explicit_python() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    let python = dir.join("python");
    let script = dir.join("encode.py");
    let ffprobe = dir.join("ffprobe");
    std::fs::write(&input, b"fixture input").unwrap();
    std::fs::write(&script, b"placeholder").unwrap();
    executable(&python, "#!/bin/sh\nexit 0\n");
    executable(&ffprobe, "#!/bin/sh\nexit 0\n");

    let result = cli()
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only", "--python"])
        .arg(&python)
        .arg("--script")
        .arg(&script)
        .arg("--ffprobe")
        .arg(&ffprobe)
        .env("PATH", &dir)
        .current_dir(&dir)
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 3, 3, "dependency", "FFmpeg not found");
    assert!(
        !output_path.exists(),
        "missing FFmpeg must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn automatically_missing_ffprobe_is_dependency_error_after_explicit_tools() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    let python = dir.join("python");
    let script = dir.join("encode.py");
    let ffmpeg = dir.join("ffmpeg");
    std::fs::write(&input, b"fixture input").unwrap();
    std::fs::write(&script, b"placeholder").unwrap();
    executable(&python, "#!/bin/sh\nexit 0\n");
    executable(&ffmpeg, "#!/bin/sh\nexit 0\n");

    let result = cli()
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only", "--python"])
        .arg(&python)
        .arg("--script")
        .arg(&script)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .env("PATH", &dir)
        .current_dir(&dir)
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 3, 3, "dependency", "FFprobe not found");
    assert!(
        !output_path.exists(),
        "missing FFprobe must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn missing_automatically_discovered_checkout_is_a_dependency_error() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    let python = dir.join("python");
    let ffmpeg = dir.join("ffmpeg");
    let ffprobe = dir.join("ffprobe");
    let isolated_cli = dir.join("tau-codecs-cli");
    std::fs::write(&input, b"fixture input").unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_tau-codecs-cli"), &isolated_cli).unwrap();
    executable(&python, "#!/bin/sh\nexit 0\n");
    executable(&ffmpeg, "#!/bin/sh\nexit 0\n");
    executable(&ffprobe, "#!/bin/sh\nexit 0\n");

    let result = Command::new(&isolated_cli)
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only", "--python"])
        .arg(&python)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--ffprobe")
        .arg(&ffprobe)
        .env("PATH", &dir)
        .env_remove("TAU_CODECS_ROOT")
        .current_dir(&dir)
        .output()
        .expect("CLI process should start");

    assert_json_error(
        result,
        3,
        3,
        "dependency",
        "Could not find Tau codecs checkout",
    );
    assert!(
        !output_path.exists(),
        "missing checkout must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn child_failure_is_unclassified_status_with_json_error_event() {
    let dir = temp_dir();
    let input = dir.join("source.mp4");
    let output_path = dir.join("out.tnv3");
    let python = dir.join("fake-python");
    let script = dir.join("encode.py");
    let ffmpeg = dir.join("ffmpeg");
    let ffprobe = dir.join("ffprobe");
    std::fs::write(&input, b"fixture input").unwrap();
    std::fs::write(&script, b"placeholder").unwrap();
    executable(
        &python,
        "#!/bin/sh\necho 'codec fixture failure' >&2\nexit 17\n",
    );
    executable(&ffmpeg, "#!/bin/sh\nexit 0\n");
    executable(&ffprobe, "#!/bin/sh\nexit 0\n");

    let result = cli()
        .arg("encode")
        .arg(&input)
        .arg(&output_path)
        .args(["--json-events", "--video-only", "--python"])
        .arg(&python)
        .arg("--script")
        .arg(&script)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--ffprobe")
        .arg(&ffprobe)
        .output()
        .expect("CLI process should start");

    assert_json_error(result, 1, 1, "unclassified", "codec fixture failure");
    assert!(
        !output_path.exists(),
        "failed child must not publish output"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
fn temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tau-cli-contract-errors-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[cfg(unix)]
fn executable(path: &std::path::Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

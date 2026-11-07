use std::process::Command;
#[test]
fn invalid_and_missing_tool_arguments_are_json_errors() {
    for args in [
        vec!["tool-identity", "--json"],
        vec!["tool-identity", "--json", "--ffmpeg", "--ffprobe"],
        vec!["tool-identity", "--ffmpeg", "foo"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["code"], 2);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn handshake_queries_only_when_requested_and_reports_selected_response() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("tau-handshake-version-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tool = dir.join("ffmpeg");
    std::fs::write(&tool, "#!/bin/sh\necho 'ffmpeg fixture 4.5'\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    let plain = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["handshake", "--json", "--ffmpeg"])
        .arg(&tool)
        .output()
        .unwrap();
    assert!(plain.status.success());
    let plain_json: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert!(plain_json["tools"]["ffmpeg"]["version"].is_null());
    let probed = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["handshake", "--json", "--probe-versions", "--ffmpeg"])
        .arg(&tool)
        .output()
        .unwrap();
    assert!(probed.status.success());
    let value: serde_json::Value = serde_json::from_slice(&probed.stdout).unwrap();
    assert!(value["tools"]["ffmpeg"]["version"].is_null());
    assert_eq!(
        value["tools"]["ffmpeg"]["version_response"],
        "ffmpeg fixture 4.5"
    );
    assert_eq!(
        value["tools"]["ffmpeg"]["version_status"],
        "response_unvalidated"
    );
    assert!(
        value["version_query_scope"]
            .as_str()
            .unwrap()
            .contains("does not bind a later execution")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn bare_relative_explicit_path_queries_the_hashed_file_not_path_lookup() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("tau-handshake-relative-{}", std::process::id()));
    let path_dir = dir.join("path");
    std::fs::create_dir_all(&path_dir).unwrap();
    let selected = dir.join("ffmpeg");
    let shadow = path_dir.join("ffmpeg");
    std::fs::write(&selected, "#!/bin/sh\necho 'usage: selected local help'\n").unwrap();
    std::fs::write(&shadow, "#!/bin/sh\necho 'PATH shadow version'\n").unwrap();
    for path in [&selected, &shadow] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args([
            "handshake",
            "--json",
            "--probe-versions",
            "--ffmpeg",
            "ffmpeg",
        ])
        .current_dir(&dir)
        .env("PATH", &path_dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["tools"]["ffmpeg"]["version_response"],
        "usage: selected local help"
    );
    assert_eq!(
        value["tools"]["ffmpeg"]["version_status"],
        "response_unvalidated"
    );
    assert!(value["tools"]["ffmpeg"]["version"].is_null());
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn hashes_explicit_regular_file_and_rejects_symlink_and_fifo() {
    use sha2::{Digest, Sha256};
    use std::{fs, os::unix::fs::symlink};
    let dir = std::env::temp_dir().join(format!("tau-identity-test-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let tool = dir.join("tool");
    fs::write(&tool, b"identified bytes").unwrap();
    let run = |p: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
            .args(["tool-identity", "--json", "--ffmpeg"])
            .arg(p)
            .output()
            .unwrap()
    };
    let ok = run(&tool);
    assert!(ok.status.success());
    let value: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(
        value["tools"]["ffmpeg"]["sha256"],
        format!("{:x}", Sha256::digest(b"identified bytes"))
    );
    assert!(value["tools"]["ffmpeg"]["version"].is_null());
    let link = dir.join("link");
    symlink(&tool, &link).unwrap();
    assert_eq!(run(&link).status.code(), Some(3));
    let fifo = dir.join("fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["tool-identity", "--json", "--ffmpeg"])
        .arg(&fifo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO open blocked");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(
        run(std::path::Path::new("/dev/null")).status.code(),
        Some(3)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_non_utf8_path_and_handles_closed_stdout() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Stdio};
    let path = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]);
    let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["tool-identity", "--json", "--ffmpeg"])
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["tool-identity", "--json", "--ffmpeg", "/dev/null"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Could not write tool identity error"));
}

use std::{
    fs,
    io::Cursor,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn invoke(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(path)
        .args(["--scope", "chunk-structure", "--json"])
        .output()
        .unwrap()
}
#[test]
fn check_reports_only_requested_scope_and_detects_corruption() {
    let dir = std::env::temp_dir().join(format!(
        "tau-scoped-check-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let file = dir.join("fixture.tnv3");
    let mut encoded = Cursor::new(Vec::new());
    tau_video::container::mux_tvc(
        &include_bytes!("../../tau-video/src/tvc/sequence-reference.tvc")[..],
        &mut encoded,
    )
    .unwrap();
    let mut data = encoded.into_inner();
    fs::write(&file, &data).unwrap();
    let pass = invoke(&file);
    assert_eq!(pass.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&pass.stdout).unwrap();
    assert_eq!(value["scope"], "chunk-structure");
    assert_eq!(value["complete"], false);
    assert_eq!(value["scan_complete"], true);
    assert_eq!(value["full_conformance_checked"], false);
    assert_eq!(value["findings_count"], 0);
    let human = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--scope", "chunk-structure"])
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&human.stdout).contains("Full conformance was not checked"));
    let mut closed = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--scope", "chunk-structure"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(closed.stdout.take());
    let closed = closed.wait_with_output().unwrap();
    assert_eq!(closed.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&closed.stderr).contains("Could not write check report"));
    let cross = |p: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
            .arg("check")
            .arg(p)
            .args(["--scope", "cross-frame", "--json"])
            .output()
            .unwrap()
    };
    let valid_cross = cross(&file);
    assert_eq!(valid_cross.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&valid_cross.stdout).unwrap();
    assert_eq!(value["scope"], "cross-frame");
    assert_eq!(value["scan_complete"], true);
    assert_eq!(value["full_conformance_checked"], false);
    let index = u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize;
    let mut wrong_pts = data.clone();
    wrong_pts[index + 4] ^= 1;
    fs::write(&file, &wrong_pts).unwrap();
    let invalid_cross = cross(&file);
    assert_eq!(invalid_cross.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&invalid_cross.stdout).unwrap();
    assert_eq!(value["scan_complete"], false);
    assert_eq!(value["findings_count"], 1);
    assert!(
        value["findings"][0]["code"]
            .as_str()
            .unwrap()
            .starts_with("E-TNV3-")
    );
    data[80] ^= 1;
    fs::write(&file, &data).unwrap();
    let bad = invoke(&file);
    assert_eq!(bad.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert!(value["findings_count"].as_u64().unwrap() > 0);
    assert_eq!(value["findings"][0]["picture_index"], 0);
    let missing = invoke(&dir.join("missing"));
    assert_eq!(missing.status.code(), Some(3));
    let no_scope = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(no_scope.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&no_scope.stdout).unwrap();
    assert_eq!(value["complete"], false);

    // Header failures use the selected scope's validation report, including
    // container-preview's existing INVALID / INDETERMINATE status contract.
    fs::write(&file, [0_u8; 64]).unwrap();
    for scope in [
        "chunk-structure",
        "cross-frame",
        "subtitle-structure",
        "container-preview",
    ] {
        let malformed = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
            .arg("check")
            .arg(&file)
            .args(["--scope", scope, "--json"])
            .output()
            .unwrap();
        assert_eq!(malformed.status.code(), Some(1), "{scope}");
        let report: serde_json::Value = serde_json::from_slice(&malformed.stdout).unwrap();
        assert_eq!(report["scope"], scope);
        assert_eq!(report["scan_complete"], false);
        assert_eq!(report["full_conformance_checked"], false);
        assert_eq!(report["findings_count"], 1);
        assert!(report["findings"][0]["offset"].is_u64());
        assert!(report.get("error").is_none());
        if scope == "container-preview" {
            assert_eq!(report["status"], "INVALID");
            assert_eq!(report["payload_decoding_checked"], false);
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quick_mode_checks_header_index_and_three_unique_chunks_without_claiming_completeness() {
    let dir = std::env::temp_dir().join(format!(
        "tau-quick-check-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let file = dir.join("fixture.tnv3");
    let mut encoded = Cursor::new(Vec::new());
    tau_video::container::mux_tvc(
        &include_bytes!("../../tau-video/src/tvc/sequence-reference.tvc")[..],
        &mut encoded,
    )
    .unwrap();
    let data = encoded.into_inner();
    fs::write(&file, &data).unwrap();
    let pass = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--quick", "--json"])
        .output()
        .unwrap();
    assert_eq!(pass.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&pass.stdout).unwrap();
    assert_eq!(value["scope"], "quick-container");
    assert_eq!(value["quick"], true);
    assert_eq!(value["complete"], false);
    assert_eq!(value["scan_complete"], false);
    assert_eq!(value["full_conformance_checked"], false);
    assert_eq!(value["pictures_attempted"], 3);
    assert_eq!(value["pictures_checked"], 3);
    assert_eq!(value["payload_decoding_checked"], false);
    assert_eq!(value["sampled_picture_indices"], serde_json::json!([0, 4, 7]));
    assert_eq!(value["attempted_picture_indices"], serde_json::json!([0, 4, 7]));
    assert_eq!(
        value["sampled_picture_indices"],
        serde_json::json!([0, 4, 7])
    );

    let mut damaged = data.clone();
    damaged[80] ^= 1;
    fs::write(&file, damaged).unwrap();
    let invalid = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--quick", "--json"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(value["status"], "INVALID");
    assert_eq!(value["pictures_attempted"], 3);
    assert_eq!(value["sampled_picture_indices"], serde_json::json!([0, 4, 7]));
    assert_eq!(value["payload_decoding_checked"], false);
    assert_eq!(value["findings"][0]["code"], "E-TNV3-016");
    assert_eq!(value["findings"][0]["offset"], 76);

    // A malformed full-size header is reported as INVALID quick JSON.
    let mut bad_header = data.clone();
    bad_header[60] ^= 1;
    fs::write(&file, bad_header).unwrap();
    let malformed = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check").arg(&file).args(["--quick", "--json"]).output().unwrap();
    assert_eq!(malformed.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(value["status"], "INVALID");
    assert_eq!(value["findings_count"], 1);
    assert_eq!(value["pictures_attempted"], 0);

    // A validly checksummed declared frame count above the reader limit is
    // INDETERMINATE, because the quick scan cannot proceed.
    let mut over_limit = data.clone();
    over_limit[28..32].copy_from_slice(&(tau_video::MAX_FRAMES + 1).to_le_bytes());
    let crc = tau_video::crc32(&over_limit[..60]);
    over_limit[60..64].copy_from_slice(&crc.to_le_bytes());
    fs::write(&file, over_limit).unwrap();
    let limited = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check").arg(&file).args(["--quick", "--json"]).output().unwrap();
    assert_eq!(limited.status.code(), Some(3), "{}", String::from_utf8_lossy(&limited.stdout));
    let value: serde_json::Value = serde_json::from_slice(&limited.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["indeterminate"]["code"], "E-TNV3-LIMIT");
    let limited_human = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--quick"])
        .output()
        .unwrap();
    assert_eq!(limited_human.status.code(), Some(3));
    let detail = String::from_utf8_lossy(&limited_human.stderr);
    assert!(detail.contains("INDETERMINATE:"), "{detail}");
    assert!(detail.contains("Too many pictures"), "{detail}");

    fs::write(&file, b"short").unwrap();
    let short = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(&file)
        .args(["--quick", "--json"])
        .output()
        .unwrap();
    assert_eq!(short.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&short.stdout).unwrap();
    assert_eq!(value["scope"], "quick-container");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-001");

    // An input open failure is an indeterminate scoped report, not a generic
    // CLI error; no picture sample was attempted and the scope remains clear.
    let missing = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(dir.join("missing.tnv3"))
        .args(["--quick", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(value["scope"], "quick-container");
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["pictures_attempted"], 0);
    assert_eq!(value["findings_count"], 0);
    assert_eq!(value["indeterminate"]["code"], "E-TNV3-IO");

    fs::remove_dir_all(dir).unwrap();
}

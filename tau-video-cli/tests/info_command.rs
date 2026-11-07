use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn info_json_reports_scope_and_failures() {
    let dir = std::env::temp_dir().join(format!(
        "tau-cli-info-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("video.tnv3");
    let mut bytes = vec![0u8; 100];
    bytes[0..4].copy_from_slice(b"TNV3");
    bytes[8..10].copy_from_slice(&16u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&16u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&24u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[20] = 1;
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&84u32.to_le_bytes());
    let crc = tau_video::crc32(&bytes[..60]);
    bytes[60..64].copy_from_slice(&crc.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    let cli = env!("CARGO_BIN_EXE_tau-codecs-cli");
    let invoke = |p: &std::path::Path| {
        Command::new(cli)
            .arg("info")
            .arg(p)
            .arg("--json")
            .output()
            .unwrap()
    };
    let ok = invoke(&path);
    assert!(ok.status.success());
    let report: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(report["info"]["width"], 16);
    assert_eq!(report["validation"]["index_entries_checked"], false);
    assert_eq!(report["validation"]["chunks_checked"], false);
    bytes[60] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let bad = invoke(&path);
    assert_eq!(bad.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(report["error"]["code"], 1);
    assert_eq!(report["error"]["kind"], "format");
    fs::write(&path, b"TNV3").unwrap();
    let truncated = invoke(&path);
    assert_eq!(truncated.status.code(), Some(1));
    let usage = Command::new(cli)
        .arg("info")
        .arg(&path)
        .arg("--json")
        .arg("extra")
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(report["error"]["code"], 2);
    assert_eq!(report["error"]["kind"], "usage");
    let missing = invoke(&dir.join("missing.tnv3"));
    assert_eq!(missing.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(report["error"]["code"], 3);
    assert_eq!(report["error"]["kind"], "io");
    fs::remove_dir_all(dir).unwrap();
}

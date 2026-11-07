use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn minimal_tnv3(with_subtitles: bool, malformed: bool) -> Vec<u8> {
    let area = if with_subtitles {
        // One eng track; one 2x1 variant; one event; one glyph.
        let mut a = vec![0; 110];
        a[0] = 1;
        a[4..7].copy_from_slice(b"eng");
        a[9] = 1;
        a[60] = 2;
        a[61] = 1;
        put32(&mut a, 64, 80);
        put32(&mut a, 68, 22);
        put32(&mut a, 72, 104);
        put32(&mut a, 76, 6);
        put32(&mut a, 80, 1);
        put32(&mut a, 84, 10);
        put32(&mut a, 88, if malformed { 10 } else { 20 });
        put16(&mut a, 92, 20);
        put16(&mut a, 94, 30);
        put16(&mut a, 98, 1);
        put16(&mut a, 100, 0);
        put16(&mut a, 104, 1);
        a[106] = 2;
        a[107] = 1;
        a[108] = 2;
        a[109] = 0x0f;
        a
    } else {
        Vec::new()
    };
    let mut file = vec![0; 80 + area.len()];
    file[..4].copy_from_slice(b"TNV3");
    put16(&mut file, 4, 0);
    put16(&mut file, 6, if with_subtitles { 4 } else { 0 });
    put16(&mut file, 8, 16);
    put16(&mut file, 10, 16);
    put32(&mut file, 12, 1);
    put32(&mut file, 16, 1);
    file[20] = 1;
    put32(&mut file, 28, 1);
    put32(&mut file, 40, 64);
    if with_subtitles {
        put32(&mut file, 44, 80);
        put32(&mut file, 48, area.len() as u32);
        file[80..].copy_from_slice(&area);
    }
    let crc = tau_video::crc32(&file[..60]);
    put32(&mut file, 60, crc);
    file
}

fn invoke(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["check"])
        .arg(path)
        .args(["--scope", "subtitle-structure", "--json"])
        .output()
        .unwrap()
}

#[test]
fn subtitle_scope_reports_valid_malformed_and_absent_areas() {
    let dir = std::env::temp_dir().join(format!(
        "tau-subtitle-check-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();

    let valid_path = dir.join("valid.tnv3");
    fs::write(&valid_path, minimal_tnv3(true, false)).unwrap();
    let valid = invoke(&valid_path);
    assert_eq!(
        valid.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
    assert_eq!(json["scope"], "subtitle-structure");
    assert_eq!(json["tracks_checked"], 1);
    assert_eq!(json["variants_checked"], 1);
    assert_eq!(json["events_checked"], 1);
    assert_eq!(json["glyphs_checked"], 1);
    assert_eq!(json["full_conformance_checked"], false);

    let malformed_path = dir.join("malformed.tnv3");
    fs::write(&malformed_path, minimal_tnv3(true, true)).unwrap();
    let malformed = invoke(&malformed_path);
    assert_eq!(malformed.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(json["findings"][0]["code"], "E-TNV3-SUB");
    assert!(
        json["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("end must be after start")
    );
    assert_eq!(json["scan_complete"], false);

    let absent_path = dir.join("no-subtitles.tnv3");
    fs::write(&absent_path, minimal_tnv3(false, false)).unwrap();
    let absent = invoke(&absent_path);
    assert_eq!(absent.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&absent.stdout).unwrap();
    assert_eq!(json["tracks_checked"], 0);
    assert_eq!(json["variants_checked"], 0);
    assert_eq!(json["events_checked"], 0);
    assert_eq!(json["glyphs_checked"], 0);
    let large_path = dir.join("large.tnv3");
    let mut large_header = minimal_tnv3(true, false);
    let area_len = tau_video::MAX_SUBTITLE_AREA_BYTES + 1;
    put32(&mut large_header, 48, area_len);
    let crc = tau_video::crc32(&large_header[..60]);
    put32(&mut large_header, 60, crc);
    fs::write(&large_path, &large_header[..80]).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&large_path)
        .unwrap()
        .set_len(80 + u64::from(area_len))
        .unwrap();
    let too_large = invoke(&large_path);
    assert_eq!(too_large.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&too_large.stdout).unwrap();
    assert_eq!(json["scope"], "subtitle-structure");
    assert_eq!(json["scan_complete"], false);
    assert_eq!(json["findings_count"], 0);
    assert_eq!(json["limit"]["code"], "E-TNV3-LIMIT");
    fs::remove_dir_all(dir).unwrap();
}

#![cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(dead_code, unused_imports)
)]
use std::{
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn base() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    tau_video::container::mux_tvc(
        &include_bytes!("../../tau-video/src/tvc/sequence-reference.tvc")[..],
        &mut out,
    )
    .unwrap();
    out.into_inner()
}

fn subtitle_area(malformed: bool) -> Vec<u8> {
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
    a[92..94].copy_from_slice(&20u16.to_le_bytes());
    a[94..96].copy_from_slice(&30u16.to_le_bytes());
    a[98..100].copy_from_slice(&1u16.to_le_bytes());
    a[104..106].copy_from_slice(&1u16.to_le_bytes());
    a[106] = 2;
    a[107] = 1;
    a[108] = 2;
    a[109] = 0x0f;
    a
}

fn with_subtitle(mut file: Vec<u8>, malformed: bool) -> Vec<u8> {
    let at = file.len();
    let area = subtitle_area(malformed);
    file.extend_from_slice(&area);
    let flags = u16::from_le_bytes([file[6], file[7]]) | 4;
    file[6..8].copy_from_slice(&flags.to_le_bytes());
    put32(&mut file, 44, at as u32);
    put32(&mut file, 48, area.len() as u32);
    let crc = tau_video::crc32(&file[..60]);
    put32(&mut file, 60, crc);
    file
}

fn run(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(path)
        .args(["--scope", "container-preview", "--json"])
        .output()
        .unwrap()
}

fn run_bounded(path: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .arg("check")
        .arg(path)
        .args(["--scope", "container-preview", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("container-preview blocked opening {:?}", path);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

struct ToggleFailure {
    inner: Cursor<Vec<u8>>,
    fail: Arc<AtomicBool>,
}
impl Read for ToggleFailure {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        self.inner.read(buf)
    }
}
impl Seek for ToggleFailure {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn combined_scope_distinguishes_valid_invalid_and_indeterminate() {
    let dir = std::env::temp_dir().join(format!(
        "tau-preview-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let valid = dir.join("valid.tnv3");
    fs::write(&valid, with_subtitle(base(), false)).unwrap();
    let output = run(&valid);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "VALID");
    assert_eq!(value["scan_complete"], true);
    assert_eq!(value["full_conformance_checked"], false);
    assert_eq!(value["payload_decoding_checked"], false);
    assert_eq!(value["input_stability_required"], true);
    assert_eq!(value["findings_count"], 0);

    let bad_sub = dir.join("invalid-subtitle.tnv3");
    fs::write(&bad_sub, with_subtitle(base(), true)).unwrap();
    let output = run(&bad_sub);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INVALID");
    assert_eq!(value["findings_count"], 1);
    assert_eq!(value["findings"][0]["code"], "E-TNV3-SUB");
    assert!(value["pictures_checked"].as_u64().unwrap() > 0);

    let bad_chunk = dir.join("invalid-chunk.tnv3");
    let mut chunk = with_subtitle(base(), false);
    chunk[80] = 9;
    fs::write(&bad_chunk, chunk).unwrap();
    let output = run(&bad_chunk);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INVALID");
    assert_eq!(value["findings_count"], 1);
    assert!(
        value["findings"][0]["code"]
            .as_str()
            .unwrap()
            .starts_with("E-TNV3-")
    );

    let too_large = dir.join("indeterminate-limit.tnv3");
    let mut prefix = base();
    let sub_at = prefix.len() as u32;
    let extra = tau_video::MAX_SUBTITLE_AREA_BYTES + 1;
    let flags = u16::from_le_bytes([prefix[6], prefix[7]]) | 4;
    prefix[6..8].copy_from_slice(&flags.to_le_bytes());
    put32(&mut prefix, 44, sub_at);
    put32(&mut prefix, 48, extra);
    let crc = tau_video::crc32(&prefix[..60]);
    put32(&mut prefix, 60, crc);
    fs::write(&too_large, &prefix).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&too_large)
        .unwrap()
        .set_len(u64::from(sub_at) + u64::from(extra))
        .unwrap();
    let output = run(&too_large);
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-LIMIT");

    let io = run(&dir.join("missing.tnv3"));
    assert_eq!(io.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&io.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-IO");

    let fail = Arc::new(AtomicBool::new(false));
    let backing = ToggleFailure {
        inner: Cursor::new(base()),
        fail: Arc::clone(&fail),
    };
    let mut reader = tau_video::Tnv3Reader::open(backing).unwrap();
    fail.store(true, Ordering::Relaxed);
    let report = tau_video::check_container_preview(&mut reader);
    assert_eq!(
        report.status,
        tau_video::ContainerPreviewStatus::Indeterminate
    );
    assert_eq!(report.findings[0].code, "E-TNV3-IO");

    let short = dir.join("short.tnv3");
    fs::write(&short, b"TNV3").unwrap();
    let output = run(&short);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INVALID");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-001");

    let directory = dir.join("directory");
    fs::create_dir(&directory).unwrap();
    let output = run(&directory);
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-NONREGULAR");

    let device = run(std::path::Path::new("/dev/null"));
    assert_eq!(device.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&device.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-NONREGULAR");

    #[cfg(unix)]
    {
        let fifo = dir.join("input.fifo");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        let output = run_bounded(&fifo);
        assert_eq!(output.status.code(), Some(3));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "INDETERMINATE");
        assert_eq!(value["findings"][0]["code"], "E-TNV3-NONREGULAR");

        let link = dir.join("link.tnv3");
        std::os::unix::fs::symlink(&valid, &link).unwrap();
        let output = run_bounded(&link);
        assert_eq!(output.status.code(), Some(3));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["status"], "INDETERMINATE");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported_target_fails_closed_as_indeterminate() {
    let dir = std::env::temp_dir().join(format!(
        "tau-preview-unsupported-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("valid.tnv3");
    fs::write(&path, base()).unwrap();
    let output = run(&path);
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "INDETERMINATE");
    assert_eq!(value["findings"][0]["code"], "E-TNV3-UNSUPPORTED-OPEN");
    fs::remove_dir_all(dir).unwrap();
}

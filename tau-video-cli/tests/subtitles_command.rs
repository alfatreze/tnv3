//! `subtitles` command against the repository's draft 1.5/1.6 vectors.
use std::{fs, path::PathBuf, process::Command};

fn vector(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/vectors/tnv3_15_16").join(format!("{name}.tnv3"))
}
fn run(args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli")).arg("subtitles").args(args).output().unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).into_owned())
}

#[test]
fn lists_tracks_variants_events_and_refuses_bad_files() {
    let v = vector("ok15_w1_w2_two_sizes");
    let (code, out) = run(&[v.to_str().unwrap(), "--json", "--events", "--variant", "1"]);
    assert_eq!(code, 0, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    let variants = &j["tracks"][0]["variants"];
    assert_eq!((variants[0]["outline_w"].as_u64(), variants[1]["outline_w"].as_u64()), (Some(1), Some(2)));
    assert_eq!((variants[1]["cell_w"].as_u64(), variants[1]["cell_h"].as_u64()), (Some(9), Some(12)));
    assert_eq!(j["tracks"][0]["language"], "eng");
    assert_eq!(j["events"].as_array().unwrap().len(), 2);
    assert_eq!(j["events"][1]["refs"], serde_json::json!([1, "break", 0]));
    assert_eq!(j["events_of"]["variant"], 1);
    let (code, _) = run(&[v.to_str().unwrap(), "--variant", "5"]);
    assert_eq!(code, 2, "no such variant");
    let (code, out) = run(&[vector("bad15_plane_not_dilation").to_str().unwrap(), "--json"]);
    assert_eq!(code, 1, "{out}");
    let (code, _) = run(&["/no/such/file.tnv3"]);
    assert_eq!(code, 3);
    assert_eq!(run(&[]).0, 2);
}

#[test]
fn render_writes_a_ppm_once_and_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("tau-subs-render-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    let out = dir.join("f.ppm");
    let v = vector("ok15_w1_w2_two_sizes");
    let (code, text) = run(&[v.to_str().unwrap(), "--json", "--render", "0.02", out.to_str().unwrap()]);
    assert_eq!(code, 0, "{text}");
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(j["rendered"]["active_events"], 1, "only the first event (samples 0..4000) is active at sample 960");
    let bytes = fs::read(&out).unwrap();
    let header = b"P6\n800 720\n255\n";
    assert_eq!(&bytes[..header.len()], header);
    assert_eq!(bytes.len(), header.len() + 800 * 720 * 3);
    assert!(bytes[header.len()..].iter().any(|&b| b != 24), "something was drawn over the background");
    assert_eq!(run(&[v.to_str().unwrap(), "--render", "0.02", out.to_str().unwrap()]).0, 2, "output exists");
    fs::remove_dir_all(&dir).unwrap();
}

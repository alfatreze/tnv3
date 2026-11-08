//! Runs the repository's own draft 1.5/1.6 vectors (`tests/vectors/tnv3_15_16`, built by
//! `tests/vectors/make_vectors_15_16.py`) through the Rust checker. The directory is part of this repository, so a
//! missing directory or file is a failure, not a skip. Each file's SHA-256 must match `expected.txt`.
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/vectors/tnv3_15_16")
}

fn entries() -> Vec<(String, bool, String)> {
    let text = fs::read_to_string(dir().join("expected.txt")).expect("tests/vectors/tnv3_15_16/expected.txt exists");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next().unwrap().to_owned();
            let valid = match it.next().unwrap() {
                "valid" => true,
                "invalid" => false,
                other => panic!("bad verdict {other}"),
            };
            let sha = it.next().unwrap().strip_prefix("sha256=").expect("sha256= field").to_owned();
            (name, valid, sha)
        })
        .collect()
}

#[test]
fn vector_files_match_their_recorded_hashes() {
    let all = entries();
    assert!(all.len() >= 18, "expected the full vector set, found {}", all.len());
    for (name, _, sha) in all {
        let bytes = fs::read(dir().join(format!("{name}.tnv3"))).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(got, sha, "{name} changed: regenerate with make_vectors_15_16.py and review the diff");
    }
}

#[test]
fn rust_checker_accepts_valid_and_rejects_every_bad_vector() {
    for (name, valid, _) in entries() {
        let path = dir().join(format!("{name}.tnv3"));
        let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
            .args(["check"])
            .arg(&path)
            .args(["--scope", "container-preview", "--json"])
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{name}: {e}"));
        let findings = json["findings_count"].as_u64().unwrap();
        if valid {
            assert_eq!((out.status.code(), findings), (Some(0), 0), "{name} should be valid: {json}");
        } else {
            assert_eq!(out.status.code(), Some(1), "{name} must be rejected: {json}");
            assert!(findings >= 1, "{name} must report a finding: {json}");
        }
    }
}

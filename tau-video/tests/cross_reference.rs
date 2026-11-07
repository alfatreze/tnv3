//! Optional local differential check; no media vectors are included in this repository.
use std::{fs, process::Command};
#[test]
fn reference_positive_vectors_agree_with_partial_cross_frame_check() {
    let (Some(dir), Some(tool)) = (std::env::var_os("TAU_VIDEO_VECTOR_DIR"),std::env::var_os("TAU_TEST_TNV3")) else { return; };
    let mut checked = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.file_name().unwrap_or_default().to_string_lossy().starts_with('p') || path.extension().is_none_or(|x| x != "tnv3") { continue; }
        let c = Command::new(&tool).arg("check").arg(&path).output().unwrap();
        assert!(c.status.success(), "reference rejected {}: {}",path.display(),String::from_utf8_lossy(&c.stderr));
        let mut reader = tau_video::Tnv3Reader::open(fs::File::open(&path).unwrap()).unwrap();
        reader.check_cross_frame().unwrap_or_else(|e| panic!("Rust rejected {}: {e}", path.display()));
        tau_video::check_subtitle_structure(&mut reader).unwrap_or_else(|e| panic!("Rust subtitle check rejected {}: {e}", path.display()));
        checked += 1;
    }
    assert!(checked > 0);
}

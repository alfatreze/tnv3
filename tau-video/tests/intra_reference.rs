//! Optional local cross-check against the standalone C decoder and local vectors.
use std::{fs, process::Command};
use tau_video::tvc::frame::decode_intra_picture;
#[test]
fn first_intra_pictures_match_local_reference() {
    let (Some(dir), Some(decoder)) = (
        std::env::var_os("TAU_TVC_VECTOR_DIR"),
        std::env::var_os("TAU_TVC_DECODER"),
    ) else {
        return;
    };
    let tmp = std::env::temp_dir().join(format!("tau-video-intra-{}.y4m", std::process::id()));
    let mut tested = 0;
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy();
        if !name.starts_with('v') || p.extension().and_then(|v| v.to_str()) != Some("tvc") {
            continue;
        }
        let b = fs::read(&p).unwrap();
        let w = u16::from_le_bytes([b[4], b[5]]);
        let h = u16::from_le_bytes([b[6], b[7]]);
        let start = if &b[..4] == b"TVC1" { 20 } else { 16 };
        let len = u32::from_le_bytes(b[start..start + 4].try_into().unwrap()) as usize;
        let frame = decode_intra_picture(&b[start + 4..start + 4 + len], w, h).unwrap();
        let ok = Command::new(&decoder)
            .arg(&p)
            .arg(&tmp)
            .status()
            .unwrap()
            .success();
        assert!(ok, "{name}");
        let out = fs::read(&tmp).unwrap();
        let hdr = out.windows(6).position(|v| v == b"FRAME\n").unwrap() + 6;
        let bytes: Vec<_> = frame
            .y
            .into_iter()
            .chain(frame.cb)
            .chain(frame.cr)
            .collect();
        assert_eq!(bytes, &out[hdr..hdr + bytes.len()], "{name}");
        tested += 1;
    }
    let _ = fs::remove_file(tmp);
    assert!(tested > 0);
    eprintln!("Compared {tested} first pictures");
}

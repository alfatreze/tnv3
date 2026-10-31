//! Optional full-picture comparison against the local C decoder.
use std::{fs, process::Command};
use tau_video::tvc::frame::{Frame420, PictureDecoder};
fn bytes(f: Frame420) -> Vec<u8> {
    f.y.into_iter().chain(f.cb).chain(f.cr).collect()
}
#[test]
fn pictures_match_local_c_reference() {
    let (Some(dir), Some(decoder)) = (
        std::env::var_os("TAU_TVC_VECTOR_DIR"),
        std::env::var_os("TAU_TVC_DECODER"),
    ) else {
        return;
    };
    let tmp = std::env::temp_dir().join(format!("tau-video-full-{}.y4m", std::process::id()));
    let mut total = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.starts_with('v') || path.extension().and_then(|x| x.to_str()) != Some("tvc") {
            continue;
        }
        let b = fs::read(&path).unwrap();
        let w = u16::from_le_bytes([b[4], b[5]]);
        let h = u16::from_le_bytes([b[6], b[7]]);
        let n = u32::from_le_bytes(b[12..16].try_into().unwrap());
        let mut pos = if &b[..4] == b"TVC1" { 20 } else { 16 };
        let mut decoder_state = PictureDecoder::new(w, h).unwrap();
        let mut output = Vec::new();
        for _ in 0..n {
            let len = u32::from_le_bytes(b[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            let picture = &b[pos..pos + len];
            pos += len;
            if let Some(decoded) = decoder_state
                .decode(picture)
                .unwrap_or_else(|e| panic!("{name} picture {}: {e}", output.len()))
            {
                output.push(bytes(decoded));
            }
        }
        if let Some(frame) = decoder_state.finish() {
            output.push(bytes(frame));
        }
        assert!(
            Command::new(&decoder)
                .arg(&path)
                .arg(&tmp)
                .status()
                .unwrap()
                .success()
        );
        let reference = fs::read(&tmp).unwrap();
        let mut at = reference.windows(6).position(|v| v == b"FRAME\n").unwrap();
        for (i, frame) in output.iter().enumerate() {
            assert_eq!(&reference[at..at + 6], b"FRAME\n");
            at += 6;
            assert_eq!(
                frame.as_slice(),
                &reference[at..at + frame.len()],
                "{name} display frame {i}"
            );
            at += frame.len();
            total += 1;
        }
    }
    let _ = fs::remove_file(tmp);
    assert!(total > 0);
    eprintln!("Compared {total} frames");
}

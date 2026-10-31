//! Optional independent C-decoder comparison for generated encoder output.
use std::{fs, process::Command};
use tau_video::tvc::{
    encoder::encode_intra_picture,
    frame::{Frame420, decode_intra_picture},
};
#[test]
fn generated_intra_pictures_match_c_decoder() {
    let Some(decoder) = std::env::var_os("TAU_TVC_DECODER") else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tau-video-encode-{}", std::process::id()));
    let tvc = base.with_extension("tvc");
    let y4m = base.with_extension("y4m");
    for (w, h) in [(16u16, 16u16), (31, 29), (64, 48)] {
        let (cw, ch) = (usize::from(w).div_ceil(2), usize::from(h).div_ceil(2));
        let frame = Frame420 {
            width: w,
            height: h,
            y: (0..usize::from(w) * usize::from(h))
                .map(|i| ((i * 7 + i / usize::from(w) * 11) % 256) as u8)
                .collect(),
            cb: (0..cw * ch).map(|i| ((i * 9 + 32) % 256) as u8).collect(),
            cr: (0..cw * ch).map(|i| ((i * 5 + 100) % 256) as u8).collect(),
        };
        for qp in [10, 25, 40, 51] {
            let picture = encode_intra_picture(&frame, qp).unwrap();
            let mut file = b"TVC1".to_vec();
            file.extend_from_slice(&w.to_le_bytes());
            file.extend_from_slice(&h.to_le_bytes());
            file.extend_from_slice(&24u16.to_le_bytes());
            file.extend_from_slice(&1u16.to_le_bytes());
            file.extend_from_slice(&1u32.to_le_bytes());
            file.extend_from_slice(&[1, 0, 0, 0]);
            file.extend_from_slice(&(picture.len() as u32).to_le_bytes());
            file.extend_from_slice(&picture);
            fs::write(&tvc, file).unwrap();
            assert!(
                Command::new(&decoder)
                    .arg(&tvc)
                    .arg(&y4m)
                    .status()
                    .unwrap()
                    .success()
            );
            let output = fs::read(&y4m).unwrap();
            let start = output.windows(6).position(|v| v == b"FRAME\n").unwrap() + 6;
            let decoded = decode_intra_picture(&picture, w, h).unwrap();
            let pixels: Vec<_> = decoded
                .y
                .into_iter()
                .chain(decoded.cb)
                .chain(decoded.cr)
                .collect();
            assert_eq!(pixels, &output[start..], "{w}x{h} qp {qp}");
        }
    }
    let _ = fs::remove_file(tvc);
    let _ = fs::remove_file(y4m);
}

#[test]
fn generated_stream_round_trips_and_matches_c_when_available() {
    use std::io::Cursor;
    use tau_video::tvc::{encoder::encode_y4m, stream::decode_y4m};
    let mut source = b"YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for k in 0..3u32 {
        source.extend_from_slice(b"FRAME\n");
        source.extend((0..1536u32).map(|i| ((i * 7 + k * 11) % 256) as u8));
    }
    let mut encoded = Cursor::new(Vec::new());
    let summary = encode_y4m(Cursor::new(source), &mut encoded, 25).unwrap();
    assert_eq!(summary.frames, 3);
    assert_eq!(summary.level, 1);
    let bytes = encoded.into_inner();
    let mut rust_output = Vec::new();
    decode_y4m(&bytes[..], &mut rust_output).unwrap();
    if let Some(decoder) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-video-stream-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, &bytes).unwrap();
        assert!(
            Command::new(decoder)
                .arg(&tvc)
                .arg(&y4m)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(rust_output, fs::read(&y4m).unwrap());
        let _ = fs::remove_file(tvc);
        let _ = fs::remove_file(y4m);
    }
}

#[test]
fn static_prediction_is_bounded_and_decodes_like_c() {
    use std::io::Cursor;
    use tau_video::tvc::{encoder::encode_y4m, stream::decode_y4m};
    let mut source = b"YUV4MPEG2 W16 H16 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for y in [128u8, 130, 160] {
        source.extend_from_slice(b"FRAME\n");
        source.extend(vec![y; 256]);
        source.extend(vec![128u8; 128]);
    }
    let mut out = Cursor::new(Vec::new());
    let summary = encode_y4m(Cursor::new(source), &mut out, 20).unwrap();
    assert_eq!(summary.predicted, 1);
    let bytes = out.into_inner();
    let mut at = 20;
    let mut kinds = Vec::new();
    for _ in 0..3 {
        let n = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        kinds.push(bytes[at]);
        at += n;
    }
    assert_eq!(kinds, [0, 1, 0]);
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    if let Some(decoder) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-video-static-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, &bytes).unwrap();
        assert!(
            Command::new(decoder)
                .arg(&tvc)
                .arg(&y4m)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(rust, fs::read(&y4m).unwrap());
        let _ = fs::remove_file(tvc);
        let _ = fs::remove_file(y4m);
    }
}

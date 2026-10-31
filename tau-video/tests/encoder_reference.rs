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

#[test]
fn translated_frames_use_motion_and_match_c_decoder() {
    use std::io::Cursor;
    use tau_video::tvc::{
        encoder::{encode_intra_picture, encode_y4m},
        frame::{Frame420, decode_intra_picture},
        stream::decode_y4m,
    };
    fn shift(src: &[u8], w: usize, h: usize, dx: i32, dy: i32) -> Vec<u8> {
        let mut out = vec![0; src.len()];
        for y in 0..h {
            for x in 0..w {
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                out[y * w + x] = src[sy * w + sx];
            }
        }
        out
    }
    let original = Frame420 {
        width: 32,
        height: 32,
        y: (0..1024)
            .map(|i| ((i * 13 + i / 32 * 17 + (i % 32) * (i / 32) * 3) % 256) as u8)
            .collect(),
        cb: (0..256).map(|i| ((i * 11) % 256) as u8).collect(),
        cr: (0..256).map(|i| ((i * 7 + 80) % 256) as u8).collect(),
    };
    let reference =
        decode_intra_picture(&encode_intra_picture(&original, 20).unwrap(), 32, 32).unwrap();
    let mut generated = Vec::new();
    let mut current = reference;
    for (dx, dy) in [(2, 0), (-2, 2), (-2, -2), (2, -2)] {
        current = Frame420 {
            width: 32,
            height: 32,
            y: shift(&current.y, 32, 32, dx, dy),
            cb: shift(&current.cb, 16, 16, dx / 2, dy / 2),
            cr: shift(&current.cr, 16, 16, dx / 2, dy / 2),
        };
        generated.push(current.clone());
    }
    let mut source = b"YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for f in std::iter::once(&original).chain(generated.iter()) {
        source.extend_from_slice(b"FRAME\n");
        source.extend_from_slice(&f.y);
        source.extend_from_slice(&f.cb);
        source.extend_from_slice(&f.cr);
    }
    let mut target = Cursor::new(Vec::new());
    let summary = encode_y4m(Cursor::new(source), &mut target, 20).unwrap();
    assert_eq!(summary.predicted, 4);
    let bytes = target.into_inner();
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    let mut at = rust.windows(6).position(|v| v == b"FRAME\n").unwrap();
    at += 6 + 1536;
    for f in &generated {
        assert_eq!(&rust[at..at + 6], b"FRAME\n");
        at += 6;
        let expected: Vec<_> = f.y.iter().chain(&f.cb).chain(&f.cr).copied().collect();
        assert_eq!(&rust[at..at + 1536], expected);
        at += 1536;
    }
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-move-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, bytes).unwrap();
        assert!(
            Command::new(tool)
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

#[test]
fn changed_frame_can_use_residual_p_and_match_c() {
    use std::io::Cursor;
    use tau_video::tvc::{
        encoder::{encode_intra_picture, encode_y4m},
        frame::{Frame420, decode_intra_picture},
        intra::IntraSliceDecoder,
        picture::{PictureHeader, PictureType},
        stream::decode_y4m,
    };
    let first = Frame420 {
        width: 32,
        height: 32,
        y: (0..1024)
            .map(|i| ((i * 7 + i / 32 * 13) % 200 + 24) as u8)
            .collect(),
        cb: vec![100; 256],
        cr: vec![150; 256],
    };
    let reference =
        decode_intra_picture(&encode_intra_picture(&first, 20).unwrap(), 32, 32).unwrap();
    let changed = Frame420 {
        width: 32,
        height: 32,
        y: reference.y.iter().map(|&v| v.saturating_add(12)).collect(),
        cb: reference.cb.iter().map(|&v| v.saturating_add(12)).collect(),
        cr: reference.cr.iter().map(|&v| v.saturating_add(12)).collect(),
    };
    let changed_again = Frame420 {
        width: 32,
        height: 32,
        y: reference.y.iter().map(|&v| v.saturating_add(24)).collect(),
        cb: reference.cb.iter().map(|&v| v.saturating_add(24)).collect(),
        cr: reference.cr.iter().map(|&v| v.saturating_add(24)).collect(),
    };
    let mut input = b"YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for f in [&first, &changed, &changed_again] {
        input.extend_from_slice(b"FRAME\n");
        input.extend_from_slice(&f.y);
        input.extend_from_slice(&f.cb);
        input.extend_from_slice(&f.cr);
    }
    let mut output = Cursor::new(Vec::new());
    let summary = encode_y4m(Cursor::new(input), &mut output, 20).unwrap();
    assert_eq!(summary.predicted, 2);
    let bytes = output.into_inner();
    let first_len = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    let p = 24 + first_len;
    let len = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
    let picture = &bytes[p + 4..p + 4 + len];
    assert_eq!(picture[0], 1);
    let header = PictureHeader::parse(picture, 2).unwrap();
    let slice = &picture[header.slices[0].clone()];
    let mut decoder = IntraSliceDecoder::for_picture(
        slice,
        2,
        1,
        header.qp,
        header.adaptive_quantization,
        header.transform_4x4,
        PictureType::Predictive,
    )
    .unwrap();
    let mut coded = 0;
    while let Some(m) = decoder.next_macroblock().unwrap() {
        coded |= m.coded;
    }
    assert_ne!(coded, 0, "P picture must contain residual coefficients");
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-residual-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, bytes).unwrap();
        assert!(
            Command::new(tool)
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

#[test]
fn translated_changed_frame_uses_motion_and_residual() {
    use std::io::Cursor;
    use tau_video::tvc::{
        encoder::{encode_intra_picture, encode_y4m},
        frame::{Frame420, decode_intra_picture},
        intra::IntraSliceDecoder,
        picture::{PictureHeader, PictureType},
        stream::decode_y4m,
    };
    fn shift_add(src: &[u8], w: usize, h: usize, dx: usize, dy: usize) -> Vec<u8> {
        let mut out = vec![0; src.len()];
        for y in 0..h {
            for x in 0..w {
                out[y * w + x] =
                    src[(y + dy).min(h - 1) * w + (x + dx).min(w - 1)].saturating_add(12);
            }
        }
        out
    }
    let first = Frame420 {
        width: 32,
        height: 32,
        y: (0..1024)
            .map(|i| ((i * 13 + i / 32 * 17 + (i % 32) * (i / 32) * 3) % 200 + 24) as u8)
            .collect(),
        cb: (0..256).map(|i| ((i * 11) % 180 + 30) as u8).collect(),
        cr: (0..256).map(|i| ((i * 7 + 80) % 180 + 30) as u8).collect(),
    };
    let reference =
        decode_intra_picture(&encode_intra_picture(&first, 20).unwrap(), 32, 32).unwrap();
    let changed = Frame420 {
        width: 32,
        height: 32,
        y: shift_add(&reference.y, 32, 32, 2, 2),
        cb: shift_add(&reference.cb, 16, 16, 1, 1),
        cr: shift_add(&reference.cr, 16, 16, 1, 1),
    };
    let mut input = b"YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for f in [&first, &changed] {
        input.extend_from_slice(b"FRAME\n");
        input.extend_from_slice(&f.y);
        input.extend_from_slice(&f.cb);
        input.extend_from_slice(&f.cr);
    }
    let mut output = Cursor::new(Vec::new());
    let summary = encode_y4m(Cursor::new(input), &mut output, 20).unwrap();
    assert_eq!(summary.predicted, 1);
    let bytes = output.into_inner();
    let n = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    let at = 24 + n;
    let size = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let picture = &bytes[at + 4..at + 4 + size];
    assert_eq!(picture[0], 1);
    let header = PictureHeader::parse(picture, 2).unwrap();
    let mut reader = IntraSliceDecoder::for_picture(
        &picture[header.slices[0].clone()],
        2,
        1,
        header.qp,
        header.adaptive_quantization,
        header.transform_4x4,
        PictureType::Predictive,
    )
    .unwrap();
    let mb = reader.next_macroblock().unwrap().unwrap();
    assert_eq!(mb.motion[0], [4, 4]);
    assert_ne!(mb.coded, 0);
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-motion-residual-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, bytes).unwrap();
        assert!(
            Command::new(tool)
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

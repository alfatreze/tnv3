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

#[cfg(feature = "block-motion")]
#[test]
fn block_motion_candidate_matches_c_decoder() {
    use std::io::Cursor;
    use tau_video::tvc::{
        encoder::{encode_intra_picture, encode_y4m},
        frame::{Frame420, decode_intra_picture},
        intra::IntraSliceDecoder,
        picture::{PictureHeader, PictureType},
        stream::decode_y4m,
    };
    fn shifted_blocks(src: &[u8], w: usize, h: usize, block: usize, amount: i32) -> Vec<u8> {
        let mut out = vec![0; src.len()];
        for y in 0..h {
            for x in 0..w {
                let dx = if (x / block + y / block) % 2 == 0 {
                    amount
                } else {
                    -amount
                };
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                out[y * w + x] = src[y * w + sx].saturating_add(6);
            }
        }
        out
    }
    let first = Frame420 {
        width: 32,
        height: 32,
        y: (0..1024)
            .map(|i| ((i * 13 + i / 32 * 17 + (i % 32) * (i / 32) * 3) % 180 + 30) as u8)
            .collect(),
        cb: (0..256).map(|i| ((i * 11) % 160 + 40) as u8).collect(),
        cr: (0..256).map(|i| ((i * 7 + 80) % 160 + 40) as u8).collect(),
    };
    let reference =
        decode_intra_picture(&encode_intra_picture(&first, 20).unwrap(), 32, 32).unwrap();
    let changed = Frame420 {
        width: 32,
        height: 32,
        y: shifted_blocks(&reference.y, 32, 32, 16, 2),
        cb: shifted_blocks(&reference.cb, 16, 16, 8, 1),
        cr: shifted_blocks(&reference.cr, 16, 16, 8, 1),
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
    let a = reader.next_macroblock().unwrap().unwrap();
    let b = reader.next_macroblock().unwrap().unwrap();
    assert_eq!(a.motion[0], [4, 0]);
    assert_eq!(b.motion[0], [-4, 0]);
    assert_ne!(a.coded | b.coded, 0);
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-block-{}", std::process::id()));
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

#[cfg(feature = "b-pictures")]
#[test]
fn bidirectional_group_matches_c_decoder_and_container_checker() {
    use std::io::Cursor;
    use tau_video::{
        container::mux_tvc,
        tvc::{
            encoder::try_encode_b_group,
            frame::{Frame420, decode_intra_picture},
            stream::decode_y4m,
        },
    };
    fn frame(y: u8) -> Frame420 {
        Frame420 {
            width: 32,
            height: 32,
            y: vec![y; 1024],
            cb: vec![128; 256],
            cr: vec![128; 256],
        }
    }
    let first = frame(80);
    let last = frame(176);
    let a = decode_intra_picture(
        &tau_video::tvc::encoder::encode_intra_picture(&first, 20).unwrap(),
        32,
        32,
    )
    .unwrap();
    let b = decode_intra_picture(
        &tau_video::tvc::encoder::encode_intra_picture(&last, 20).unwrap(),
        32,
        32,
    )
    .unwrap();
    let mid = Frame420 {
        width: 32,
        height: 32,
        y: a.y
            .iter()
            .zip(&b.y)
            .map(|(&x, &y)| ((u16::from(x) + u16::from(y) + 1) >> 1) as u8)
            .collect(),
        cb: vec![128; 256],
        cr: vec![128; 256],
    };
    let group = try_encode_b_group(&first, &mid, &last, 20)
        .unwrap()
        .unwrap();
    assert_eq!(group.middle_picture[0], 2);
    let intra_middle = tau_video::tvc::encoder::encode_intra_picture(&mid, 20).unwrap();
    assert!(group.middle_picture.len() < intra_middle.len());
    assert!(
        try_encode_b_group(&first, &frame(0), &last, 20)
            .unwrap()
            .is_none()
    );
    let mut bytes = b"TVC1".to_vec();
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0]);
    for p in [
        &group.first_anchor,
        &group.future_anchor,
        &group.middle_picture,
    ] {
        bytes.extend_from_slice(&(p.len() as u32).to_le_bytes());
        bytes.extend_from_slice(p);
    }
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    let start = rust.windows(6).position(|x| x == b"FRAME\n").unwrap();
    let frame_len = 1536;
    let second = start + 6 + frame_len;
    assert_eq!(&rust[second..second + 6], b"FRAME\n");
    assert_eq!(&rust[second + 6..second + 6 + 1024], &mid.y);
    let mut muxed = Cursor::new(Vec::new());
    mux_tvc(&bytes[..], &mut muxed).unwrap();
    let container = muxed.into_inner();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-bgroup-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, &bytes).unwrap();
        assert!(
            Command::new(tool)
                .arg(&tvc)
                .arg(&y4m)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(rust, fs::read(&y4m).unwrap());
        if let Some(checker) = std::env::var_os("TAU_TNV3_TOOL") {
            let tnv = base.with_extension("tnv3");
            fs::write(&tnv, &container).unwrap();
            assert!(
                Command::new(checker)
                    .arg("check")
                    .arg(&tnv)
                    .status()
                    .unwrap()
                    .success()
            );
            let _ = fs::remove_file(tnv);
        }
        let _ = fs::remove_file(tvc);
        let _ = fs::remove_file(y4m);
    }
}

#[cfg(feature = "b-pictures")]
#[test]
fn b_stream_uses_groups_and_falls_back_in_display_order() {
    use std::io::Cursor;
    use tau_video::{
        container::mux_tvc,
        tvc::{encoder::encode_y4m_b, stream::decode_y4m},
    };
    let mut input = b"YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for value in [80u8, 128, 176, 15, 240, 16, 130] {
        input.extend_from_slice(b"FRAME\n");
        input.extend(vec![value; 1024]);
        input.extend(vec![128u8; 512]);
    }
    let mut stream = Cursor::new(Vec::new());
    let summary = encode_y4m_b(Cursor::new(input), &mut stream, 20).unwrap();
    assert_eq!(summary.frames, 7);
    assert_eq!(summary.b_pictures, 1);
    let bytes = stream.into_inner();
    let mut pos = 20;
    let mut types = Vec::new();
    for _ in 0..7 {
        let n = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        types.push(bytes[pos]);
        pos += n;
    }
    assert_eq!(types, [0, 0, 2, 0, 0, 0, 0]);
    let mut rust = Vec::new();
    decode_y4m(&bytes[..], &mut rust).unwrap();
    let mut at = rust.windows(6).position(|x| x == b"FRAME\n").unwrap();
    let mut means = Vec::new();
    for _ in 0..7 {
        assert_eq!(&rust[at..at + 6], b"FRAME\n");
        at += 6;
        means.push(
            rust[at..at + 1024]
                .iter()
                .map(|&v| u32::from(v))
                .sum::<u32>()
                / 1024,
        );
        at += 1536;
    }
    assert!(means[0] < means[1] && means[1] < means[2]);
    assert!(means[3] < means[4] && means[4] > means[5]);
    let mut container = Cursor::new(Vec::new());
    mux_tvc(&bytes[..], &mut container).unwrap();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-bstream-{}", std::process::id()));
        let tvc = base.with_extension("tvc");
        let y4m = base.with_extension("y4m");
        fs::write(&tvc, &bytes).unwrap();
        assert!(
            Command::new(tool)
                .arg(&tvc)
                .arg(&y4m)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(rust, fs::read(&y4m).unwrap());
        if let Some(checker) = std::env::var_os("TAU_TNV3_TOOL") {
            let tnv = base.with_extension("tnv3");
            fs::write(&tnv, container.into_inner()).unwrap();
            assert!(
                Command::new(checker)
                    .arg("check")
                    .arg(&tnv)
                    .status()
                    .unwrap()
                    .success()
            );
            let _ = fs::remove_file(tnv);
        }
        let _ = fs::remove_file(tvc);
        let _ = fs::remove_file(y4m);
    }
}

#[cfg(feature = "b-pictures")]
#[test]
fn bidirectional_group_can_encode_residuals() {
    use tau_video::tvc::{
        encoder::{encode_intra_picture, try_encode_b_group},
        frame::{Frame420, decode_intra_picture},
        intra::IntraSliceDecoder,
        picture::{PictureHeader, PictureType},
    };
    let first = Frame420 {
        width: 32,
        height: 32,
        y: (0..1024)
            .map(|i| ((i * 13 + i / 32 * 17 + (i % 32) * (i / 32) * 3) % 180 + 30) as u8)
            .collect(),
        cb: (0..256).map(|i| ((i * 11) % 160 + 40) as u8).collect(),
        cr: (0..256).map(|i| ((i * 7 + 80) % 160 + 40) as u8).collect(),
    };
    let reference =
        decode_intra_picture(&encode_intra_picture(&first, 20).unwrap(), 32, 32).unwrap();
    let middle = Frame420 {
        width: 32,
        height: 32,
        y: reference.y.iter().map(|&x| x.saturating_add(12)).collect(),
        cb: reference.cb.iter().map(|&x| x.saturating_add(12)).collect(),
        cr: reference.cr.iter().map(|&x| x.saturating_add(12)).collect(),
    };
    let g = try_encode_b_group(&first, &middle, &first, 20)
        .unwrap()
        .expect("residual B should beat intra");
    let h = PictureHeader::parse(&g.middle_picture, 2).unwrap();
    let mut d = IntraSliceDecoder::for_picture(
        &g.middle_picture[h.slices[0].clone()],
        2,
        1,
        h.qp,
        h.adaptive_quantization,
        h.transform_4x4,
        PictureType::Bidirectional,
    )
    .unwrap();
    let mb = d.next_macroblock().unwrap().unwrap();
    assert_eq!(mb.uses, 3);
    assert_ne!(mb.coded, 0);
    let mut tvc = b"TVC1".to_vec();
    tvc.extend_from_slice(&32u16.to_le_bytes());
    tvc.extend_from_slice(&32u16.to_le_bytes());
    tvc.extend_from_slice(&24u16.to_le_bytes());
    tvc.extend_from_slice(&1u16.to_le_bytes());
    tvc.extend_from_slice(&3u32.to_le_bytes());
    tvc.extend_from_slice(&[1, 0, 0, 0]);
    for p in [&g.first_anchor, &g.future_anchor, &g.middle_picture] {
        tvc.extend_from_slice(&(p.len() as u32).to_le_bytes());
        tvc.extend_from_slice(p);
    }
    let mut rust = Vec::new();
    tau_video::tvc::stream::decode_y4m(&tvc[..], &mut rust).unwrap();
    if let Some(tool) = std::env::var_os("TAU_TVC_DECODER") {
        let base = std::env::temp_dir().join(format!("tau-bresidual-{}", std::process::id()));
        let src = base.with_extension("tvc");
        let out = base.with_extension("y4m");
        fs::write(&src, tvc).unwrap();
        assert!(
            Command::new(tool)
                .arg(&src)
                .arg(&out)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(rust, fs::read(&out).unwrap());
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(out);
    }
}

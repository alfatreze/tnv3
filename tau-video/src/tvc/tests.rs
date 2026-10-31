use super::range::{Probability, RangeDecoder, RangeEncoder};
#[test]
fn c_reference_and_round_trip() {
    let mut e = RangeEncoder::new();
    let mut p = [Probability::default(); 5];
    let mut x = 1234567u32;
    for i in 0..1000 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        let bit = x >> 31 != 0;
        if i % 7 == 0 {
            e.bypass(bit);
        } else {
            e.bit(&mut p[i % 5], bit);
        }
    }
    let encoded = e.finish();
    let expected = include_str!("reference.txt").trim();
    let actual: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(actual, expected, "C reference encoder bytes");
    let mut d = RangeDecoder::new(&encoded);
    let mut p = [Probability::default(); 5];
    let mut x = 1234567u32;
    for i in 0..1000 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        let bit = x >> 31 != 0;
        let got = if i % 7 == 0 {
            d.bypass()
        } else {
            d.bit(&mut p[i % 5])
        };
        assert_eq!(got, bit, "bin {i}");
    }
}
#[test]
fn zero_padded_short_input_is_bounded() {
    let mut d = RangeDecoder::new(&[]);
    let mut p = Probability::default();
    for _ in 0..10000 {
        let _ = d.bit(&mut p);
    }
}

#[test]
fn picture_slice_table_is_bounded() {
    use super::picture::{PictureHeader, PictureType};
    let data = [0, 30, 8, 1, 2, 0, 0, 0, 1, 0, 0, 0, 5, 6, 7, 8, 9];
    let h = PictureHeader::parse(&data, 3).unwrap();
    assert_eq!(h.kind, PictureType::Intra);
    assert_eq!(h.slices, vec![12..14, 14..15, 15..17]);
    for n in 0..12 {
        assert!(PictureHeader::parse(&data[..n], 3).is_err());
    }
    let mut bad = data;
    bad[4] = 250;
    assert!(PictureHeader::parse(&bad, 3).is_err());
    assert!(PictureHeader::parse(&data, 46).is_err());
}

#[test]
fn local_tvc_picture_headers_if_available() {
    use super::picture::PictureHeader;
    let Some(dir) = std::env::var_os("TAU_TVC_VECTOR_DIR") else {
        return;
    };
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.file_name().unwrap().to_string_lossy().starts_with('v')
            || path.extension().and_then(|s| s.to_str()) != Some("tvc")
        {
            continue;
        }
        let b = std::fs::read(path).unwrap();
        let height = u16::from_le_bytes([b[6], b[7]]);
        let rows = u8::try_from(u32::from(height).div_ceil(16)).unwrap();
        let frames = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
        let mut pos = if &b[..4] == b"TVC1" { 20 } else { 16 };
        for _ in 0..frames {
            let len = u32::from_le_bytes(b[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            PictureHeader::parse(&b[pos..pos + len], rows).unwrap();
            pos += len;
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn inverse_transforms_match_c_reference() {
    use super::transform::{inverse_4x4, inverse_8x8};
    let reference = include_bytes!("transform-reference.bin");
    let mut pos = 0;
    let mut x = 891203u32;
    for n in 0..104 {
        let mut levels = [0i16; 64];
        for v in &mut levels {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = ((x >> 16) % 601) as i16 - 300;
        }
        let large = inverse_8x8(&levels, (n % 52) as u8).unwrap();
        let small = inverse_4x4(levels[..16].try_into().unwrap(), (n % 52) as u8).unwrap();
        for v in large.iter().flatten().chain(small.iter().flatten()) {
            let expected = i32::from_le_bytes(reference[pos..pos + 4].try_into().unwrap());
            pos += 4;
            assert_eq!(*v, expected, "case {n}");
        }
    }
    assert_eq!(pos, reference.len());
    assert!(inverse_8x8(&[0; 64], 52).is_err());
}

#[test]
fn intra_prediction_matches_c_reference() {
    use super::predict::{IntraMode, intra_8x8};
    let expected = include_bytes!("pred-reference.bin");
    let mut offset = 0;
    let mut state = 78123u32;
    for _ in 0..64 {
        let mut plane = [0u8; 256];
        for p in &mut plane {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *p = (state >> 24) as u8;
        }
        for pos in 0..4 {
            let bx = (pos & 1) * 8;
            let by = (pos >> 1) * 8;
            let mut top = [0; 8];
            let mut left = [0; 8];
            if by > 0 {
                top.copy_from_slice(&plane[(by - 1) * 16 + bx..(by - 1) * 16 + bx + 8]);
            }
            if bx > 0 {
                for i in 0..8 {
                    left[i] = plane[(by + i) * 16 + bx - 1];
                }
            }
            for mode in [
                IntraMode::Dc,
                IntraMode::Vertical,
                IntraMode::Horizontal,
                IntraMode::Planar,
            ] {
                let got = intra_8x8(mode, (by > 0).then_some(&top), (bx > 0).then_some(&left));
                for v in got.into_iter().flatten() {
                    assert_eq!(v, expected[offset]);
                    offset += 1;
                }
            }
        }
    }
    assert_eq!(offset, expected.len());
}

#[test]
fn inter_prediction_matches_c_reference() {
    use super::inter::{Interpolation, ReferencePlane};
    let expected = include_bytes!("inter-reference.bin");
    let mut offset = 0;
    let mut state = 912325u32;
    for n in 0..128i32 {
        let mut plane = [0u8; 32 * 24];
        for p in &mut plane {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *p = (state >> 24) as u8;
        }
        let r = ReferencePlane::new(&plane, 32, 24, 32).unwrap();
        let origin = ((n * 7) % 32, (n * 11) % 24);
        let mv = ((n % 33) - 16, ((n * 13) % 33) - 16);
        for mode in [
            Interpolation::HalfSample,
            Interpolation::QuarterLuma,
            Interpolation::EighthChroma,
        ] {
            let result = r.predict(origin, mv, (16, 16), mode).unwrap();
            assert_eq!(
                &result,
                &expected[offset..offset + 256],
                "case {n} {mode:?}"
            );
            offset += 256;
        }
    }
    assert_eq!(offset, expected.len());
    assert!(ReferencePlane::new(&[0; 4], 10, 10, 10).is_err());
}

#[test]
fn deblocking_matches_c_reference() {
    use super::deblock::{DeblockMacroblock, deblock};
    let expected = include_bytes!("deblock-reference.bin");
    let mut pos = 0;
    let mut state = 29132u32;
    for n in 0..104usize {
        let mut mb = [DeblockMacroblock {
            intra: false,
            uses: 0,
            coded: 0,
            qp: 0,
            motion: [[0; 2]; 2],
        }; 4];
        for (j, m) in mb.iter_mut().enumerate() {
            m.intra = (n + j) % 3 == 0;
            m.uses = ((n + j) % 4) as u8;
            m.coded = ((n * 7 + j * 13) % 64) as u8;
            m.qp = ((n + j) % 52) as u8;
            for l in 0..2 {
                for k in 0..2 {
                    m.motion[l][k] = ((n * 3 + j * 11 + l * 7 + k * 5) % 25) as i16 - 12;
                }
            }
        }
        let mut y = [0u8; 1024];
        let mut u = [0u8; 256];
        let mut v = [0u8; 256];
        for plane in [&mut y[..], &mut u[..], &mut v[..]] {
            for p in plane {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                *p = 112 + ((state >> 24) % 32) as u8;
            }
        }
        deblock(&mut y, &mut u, &mut v, 32, 32, &mb, n % 2 != 0).unwrap();
        for plane in [&y[..], &u[..], &v[..]] {
            assert_eq!(plane, &expected[pos..pos + plane.len()], "case {n}");
            pos += plane.len();
        }
    }
    assert_eq!(pos, expected.len());
}

#[test]
fn invalid_deblock_input_does_not_modify_planes() {
    use super::deblock::{DeblockMacroblock, deblock};
    let mut y = [17u8; 256];
    let mut u = [18u8; 64];
    let mut v = [19u8; 64];
    let invalid = [DeblockMacroblock {
        intra: true,
        uses: 0,
        motion: [[0; 2]; 2],
        coded: 0,
        qp: 52,
    }];
    assert!(deblock(&mut y, &mut u, &mut v, 16, 16, &invalid, false).is_err());
    assert_eq!(y, [17u8; 256]);
    assert_eq!(u, [18u8; 64]);
    assert_eq!(v, [19u8; 64]);
}

#[test]
fn coefficient_blocks_match_c_reference() {
    use super::coeff::{BlockClass, CoefficientContexts, decode_block, encode_block};
    use super::range::{RangeDecoder, RangeEncoder};
    let classes = [BlockClass::Luma8, BlockClass::Chroma8, BlockClass::Luma4];
    let mut enc = RangeEncoder::new();
    let mut ctx = CoefficientContexts::default();
    let mut state = 163239u32;
    let mut blocks = Vec::new();
    for n in 0..300usize {
        let class = classes[n % 3];
        let len = if n % 3 == 2 { 16 } else { 64 };
        let mut lev = vec![0i16; len];
        for v in &mut lev {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = if (state >> 24) % 7 == 0 {
                (((state >> 8) % 601) as i32 - 300) as i16
            } else {
                0
            };
        }
        lev[n % len] = if n % 2 != 0 { -2 } else { 1 };
        encode_block(&mut enc, &mut ctx, class, &lev).unwrap();
        blocks.push((class, lev));
    }
    let data = enc.finish();
    assert_eq!(data.as_slice(), include_bytes!("coeff-reference.bin"));
    let mut dec = RangeDecoder::new(&data);
    let mut ctx = CoefficientContexts::default();
    for (class, expected) in blocks {
        let mut actual = vec![0; expected.len()];
        decode_block(&mut dec, &mut ctx, class, &mut actual).unwrap();
        assert_eq!(actual, expected);
    }
    let mut enc = RangeEncoder::new();
    let mut ctx = CoefficientContexts::default();
    assert!(encode_block(&mut enc, &mut ctx, BlockClass::Luma4, &[0; 16]).is_err());
}

#[test]
fn coefficient_extremes_and_invalid_lengths() {
    use super::coeff::{BlockClass, CoefficientContexts, decode_block, encode_block};
    use super::range::{RangeDecoder, RangeEncoder};
    let mut src = [0i16; 16];
    src[0] = i16::MIN;
    src[15] = i16::MAX;
    let mut encoder = RangeEncoder::new();
    let mut contexts = CoefficientContexts::default();
    assert!(encode_block(&mut encoder, &mut contexts, BlockClass::Luma4, &src[..15]).is_err());
    encode_block(&mut encoder, &mut contexts, BlockClass::Luma4, &src).unwrap();
    let bytes = encoder.finish();
    let mut decoder = RangeDecoder::new(&bytes);
    let mut contexts = CoefficientContexts::default();
    let mut actual = [0i16; 16];
    decode_block(&mut decoder, &mut contexts, BlockClass::Luma4, &mut actual).unwrap();
    assert_eq!(actual, src);
}

#[test]
fn macroblock_motion_prediction_matches_c_reference() {
    use super::deblock::DeblockMacroblock;
    use super::motion::predict_motion;
    let expected = include_bytes!("mvp-reference.bin");
    let mut pos = 0;
    for n in 0..512usize {
        let mut mb = [DeblockMacroblock {
            intra: false,
            uses: 0,
            motion: [[0; 2]; 2],
            coded: 0,
            qp: 0,
        }; 9];
        for (j, m) in mb.iter_mut().enumerate() {
            m.uses = ((n + j * 3) % 4) as u8;
            for l in 0..2 {
                for k in 0..2 {
                    m.motion[l][k] = ((n * 19 + j * 17 + l * 37 + k * 53) % 401) as i16 - 200;
                }
            }
        }
        let x = n % 3;
        let y = (n / 3) % 3;
        let start = if n % 2 != 0 { y } else { 0 };
        for list in 0..2 {
            let got = predict_motion(&mb, 3, 3, x, y, start, list).unwrap();
            for v in got {
                let e = i16::from_le_bytes(expected[pos..pos + 2].try_into().unwrap());
                pos += 2;
                assert_eq!(v, e, "case {n} list {list}");
            }
        }
    }
    assert_eq!(pos, expected.len());
    assert!(predict_motion(&[], 3, 3, 0, 0, 0, 0).is_err());
}

#[test]
fn signed_exp_golomb_matches_c_reference() {
    use super::range::{Probability, RangeDecoder, RangeEncoder};
    use super::signed::{MAX_MAGNITUDE, decode_signed, encode_signed};
    let mut state = 918823u32;
    let mut values = Vec::new();
    let mut e = RangeEncoder::new();
    let mut z = [Probability::default(); 2];
    let mut m = [Probability::default(); 6];
    for n in 0..1500 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let v = if (state >> 24) % 5 != 0 {
            ((state >> 8) % 4097) as i32 - 2048
        } else {
            0
        };
        encode_signed(&mut e, &mut z[n % 2], &mut m, v).unwrap();
        values.push(v);
    }
    let bytes = e.finish();
    assert_eq!(bytes.as_slice(), include_bytes!("seg-reference.bin"));
    let mut d = RangeDecoder::new(&bytes);
    let mut z = [Probability::default(); 2];
    let mut m = [Probability::default(); 6];
    for (n, v) in values.into_iter().enumerate() {
        assert_eq!(decode_signed(&mut d, &mut z[n % 2], &mut m).unwrap(), v);
    }
    let mut e = RangeEncoder::new();
    let mut p = Probability::default();
    let mut m = [p; 3];
    assert!(encode_signed(&mut e, &mut p, &mut m, MAX_MAGNITUDE + 1).is_err());
}

#[test]
fn signed_exp_golomb_limits() {
    use super::range::{Probability, RangeDecoder, RangeEncoder};
    use super::signed::{MAX_MAGNITUDE, decode_signed, encode_signed};
    let mut e = RangeEncoder::new();
    let mut z = Probability::default();
    let mut c = [Probability::default(); 3];
    for v in [-MAX_MAGNITUDE, 0, MAX_MAGNITUDE] {
        encode_signed(&mut e, &mut z, &mut c, v).unwrap();
    }
    let bytes = e.finish();
    let mut d = RangeDecoder::new(&bytes);
    let mut z = Probability::default();
    let mut c = [Probability::default(); 3];
    for v in [-MAX_MAGNITUDE, 0, MAX_MAGNITUDE] {
        assert_eq!(decode_signed(&mut d, &mut z, &mut c).unwrap(), v);
    }
    assert!(decode_signed(&mut d, &mut z, &mut []).is_err());
}

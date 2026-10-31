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

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

use std::io::Cursor;
use tau_video::{MAX_FRAMES, Tnv3Reader, crc32};
fn fixture() -> Vec<u8> {
    let mut b = vec![0u8; 100];
    b[0..4].copy_from_slice(b"TNV3");
    b[8..10].copy_from_slice(&16u16.to_le_bytes());
    b[10..12].copy_from_slice(&16u16.to_le_bytes());
    b[12..16].copy_from_slice(&24u32.to_le_bytes());
    b[16..20].copy_from_slice(&1u32.to_le_bytes());
    b[20] = 1;
    b[28..32].copy_from_slice(&1u32.to_le_bytes());
    b[40..44].copy_from_slice(&84u32.to_le_bytes());
    let c = crc32(&b[..60]);
    b[60..64].copy_from_slice(&c.to_le_bytes());
    b[84..88].copy_from_slice(&64u32.to_le_bytes());
    b
}
fn refresh(b: &mut [u8]) {
    let c = crc32(&b[..60]);
    b[60..64].copy_from_slice(&c.to_le_bytes());
}
#[test]
fn crc_known_answer() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
}
#[test]
fn header_and_index() {
    let mut r = Tnv3Reader::open(Cursor::new(fixture())).unwrap();
    assert_eq!(
        (r.info().width, r.info().height, r.info().frame_count),
        (16, 16, 1)
    );
    assert_eq!(r.index_entry(0).unwrap().offset, 64);
    assert!(r.index_entry(1).is_err());
}
#[test]
fn rejects_mutations_and_truncation() {
    let b = fixture();
    for n in 0..b.len() {
        assert!(
            Tnv3Reader::open(Cursor::new(&b[..n])).is_err(),
            "length {n}"
        );
    }
    for (at, value) in [
        (0, 0u8),
        (4, 1),
        (6, 128),
        (8, 0),
        (20, 4),
        (22, 1),
        (52, 1),
        (60, 0),
    ] {
        let mut x = b.clone();
        x[at] = value;
        if at != 60 {
            refresh(&mut x);
        }
        assert!(Tnv3Reader::open(Cursor::new(x)).is_err(), "offset {at}");
    }
    let mut x = b;
    x[28..32].copy_from_slice(&(MAX_FRAMES + 1).to_le_bytes());
    refresh(&mut x);
    assert!(matches!(
        Tnv3Reader::open(Cursor::new(x)),
        Err(tau_video::Error::LimitExceeded { .. })
    ));
}
#[test]
fn randomized_input_never_panics() {
    let mut seed = 1u32;
    for len in 0..512 {
        let mut v = vec![0; len];
        for b in &mut v {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (seed >> 24) as u8;
        }
        let _ = Tnv3Reader::open(Cursor::new(v));
    }
}
#[test]
fn local_reference_valid_headers_if_available() {
    let Some(dir) = std::env::var_os("TAU_VIDEO_VECTOR_DIR") else {
        return;
    };
    for name in [
        "p01_ip_24_crc.tnv3",
        "p02_b2_23976_crc.tnv3",
        "p03_b3_gop7_2997.tnv3",
        "p04_slices_25_mono.tnv3",
        "p05_l1_video_only.tnv3",
        "p06_crop_b2_subs.tnv3",
        "p07_long_audio_tail.tnv3",
        "p08_l3_2slices_30.tnv3",
        "p09_seek_at_reset.tnv3",
    ] {
        let p = std::path::Path::new(&dir).join(name);
        let f = std::fs::File::open(p).unwrap();
        assert!(Tnv3Reader::open(f).is_ok(), "{name}");
    }
}

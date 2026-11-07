use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::cell::Cell;
use std::rc::Rc;
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

#[test]
fn local_reference_pictures_validate_when_vectors_are_configured() {
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
        let file = std::fs::File::open(std::path::Path::new(&dir).join(name)).unwrap();
        let mut reader = Tnv3Reader::open(file).unwrap();
        for n in 0..reader.info().frame_count {
            reader
                .read_picture(n)
                .unwrap_or_else(|e| panic!("{name} picture {n}: {e}"));
        }
    }
}

#[test]
fn real_audio_vectors_cover_chunk_crc_enabled_and_disabled() {
    let Some(dir) = std::env::var_os("TAU_VIDEO_VECTOR_DIR") else {
        return;
    };
    for (name, crc_enabled) in [
        ("p01_ip_24_crc.tnv3", true),
        ("p03_b3_gop7_2997.tnv3", false),
    ] {
        let file = std::fs::File::open(std::path::Path::new(&dir).join(name)).unwrap();
        let mut reader = Tnv3Reader::open(file).unwrap();
        assert_ne!(reader.info().flags & 2, 0, "{name} must contain audio");
        assert_eq!(reader.info().flags & 1 != 0, crc_enabled, "{name}");
        for n in 0..reader.info().frame_count {
            reader
                .read_picture(n)
                .unwrap_or_else(|e| panic!("{name} picture {n}: {e}"));
        }
    }
}

fn structural_fixture() -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    tau_video::container::mux_tvc(
        &include_bytes!("../src/tvc/sequence-reference.tvc")[..],
        &mut out,
    ).unwrap();
    out.into_inner()
}

#[test]
fn structural_check_accepts_valid_embedded_sequence_with_no_findings() {
    let mut reader = Tnv3Reader::open(Cursor::new(structural_fixture())).unwrap();
    let count = reader.info().frame_count;
    let report = tau_video::check_chunk_structure(&mut reader).unwrap();
    assert_eq!(report.pictures_attempted, count);
    assert_eq!(report.pictures_checked, count);
    assert!(report.findings.is_empty());
    assert_eq!(report.findings_omitted, 0);
}

#[test]
fn structural_check_reports_second_and_last_corruption_and_continues() {
    let source = structural_fixture();
    let pristine = Tnv3Reader::open(Cursor::new(&source)).unwrap();
    let count = pristine.info().frame_count;
    assert!(count >= 3);
    for damaged_index in [1, count - 1] {
        let mut damaged = source.clone();
        let entry_at = pristine.info().index_offset as usize + damaged_index as usize * 16;
        let chunk_at = u32::from_le_bytes(damaged[entry_at..entry_at + 4].try_into().unwrap()) as usize;
        damaged[chunk_at + 16] ^= 0x01;
        let mut reader = Tnv3Reader::open(Cursor::new(damaged)).unwrap();
        let report = tau_video::check_chunk_structure(&mut reader).unwrap();
        assert_eq!(report.pictures_attempted, count);
        assert_eq!(report.pictures_checked, count - 1);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].picture_index, damaged_index);
        assert!(matches!(report.findings[0].error, tau_video::Error::Invalid { code: "E-TNV3-016", .. }));
    }
}

struct FailingReader {
    cursor: Cursor<Vec<u8>>,
    fail: Rc<Cell<bool>>,
}
impl Read for FailingReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.fail.get() && self.cursor.position() >= 64 {
            return Err(io::Error::new(io::ErrorKind::Other, "injected read failure"));
        }
        self.cursor.read(out)
    }
}
impl Seek for FailingReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> { self.cursor.seek(pos) }
}

#[test]
fn structural_check_returns_injected_io_error_without_downgrading_it() {
    let fail = Rc::new(Cell::new(false));
    let mut reader = Tnv3Reader::open(FailingReader { cursor: Cursor::new(structural_fixture()), fail: fail.clone() }).unwrap();
    fail.set(true);
    let err = tau_video::check_chunk_structure(&mut reader).unwrap_err();
    assert!(matches!(err, tau_video::Error::Io(ref e) if e.to_string() == "injected read failure"));
}

fn repeat_chunks_as_video(source: &[u8], repeats: usize) -> Vec<u8> {
    let mut reader = Tnv3Reader::open(Cursor::new(source)).unwrap();
    let source_count = reader.info().frame_count;
    let source_index = reader.info().index_offset as usize;
    let entries = (0..source_count).map(|i| reader.index_entry(i).unwrap()).collect::<Vec<_>>();
    let mut out = source[..64].to_vec();
    let mut offsets = Vec::new();
    for _ in 0..repeats {
        for i in 0..source_count {
            let start = entries[i as usize].offset as usize;
            let end = if i + 1 < source_count { entries[i as usize + 1].offset as usize } else { source_index };
            offsets.push(out.len() as u32);
            out.extend_from_slice(&source[start..end]);
        }
    }
    let new_index = out.len() as u32;
    for rep in 0..repeats {
        for (i, entry) in entries.iter().enumerate() {
            out.extend_from_slice(&offsets[rep * source_count as usize + i].to_le_bytes());
            out.extend_from_slice(&entry.presentation_sample.to_le_bytes());
            out.extend_from_slice(&entry.first_tac.to_le_bytes());
            out.extend_from_slice(&entry.flags_and_seek.to_le_bytes());
        }
    }
    let total = source_count * repeats as u32;
    out[28..32].copy_from_slice(&total.to_le_bytes());
    out[40..44].copy_from_slice(&new_index.to_le_bytes());
    let checksum = crc32(&out[..60]);
    out[60..64].copy_from_slice(&checksum.to_le_bytes());
    out
}

#[test]
fn structural_check_caps_retained_findings_and_counts_omissions() {
    let mut bytes = repeat_chunks_as_video(&structural_fixture(), 9);
    let mut header = Tnv3Reader::open(Cursor::new(&bytes)).unwrap();
    let chunks = (0..header.info().frame_count)
        .map(|i| header.index_entry(i).unwrap().offset as usize)
        .collect::<Vec<_>>();
    drop(header);
    for chunk in chunks {
        bytes[chunk + 10] |= 0x80; // invalid reserved chunk flag
    }
    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    let report = tau_video::check_chunk_structure(&mut reader).unwrap();
    assert_eq!(report.pictures_attempted, 72);
    assert_eq!(report.pictures_checked, 0);
    assert_eq!(report.findings.len(), tau_video::MAX_CHUNK_STRUCTURE_FINDINGS);
    assert_eq!(report.findings_omitted, 72 - tau_video::MAX_CHUNK_STRUCTURE_FINDINGS as u32);
    assert_eq!(report.findings[0].picture_index, 0);
}

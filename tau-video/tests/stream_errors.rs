use std::io::{self, Read, Write};
use tau_video::tvc::stream::{decode_y4m, decode_y4m_detailed, DetailedStreamError, StreamError};

fn header(magic: &[u8; 4], width: u16, height: u16, num: u16, den: u16, frames: u32) -> Vec<u8> {
    let mut bytes = Vec::from(&magic[..]);
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&num.to_le_bytes());
    bytes.extend_from_slice(&den.to_le_bytes());
    bytes.extend_from_slice(&frames.to_le_bytes());
    bytes
}

#[test]
fn legacy_api_retains_exhaustive_error_shape_and_classification() {
    let mut output = Vec::new();
    let err = decode_y4m(&b"nope"[..], &mut output).unwrap_err();
    assert!(matches!(err, StreamError::Io(_)));
    let err = decode_y4m(&header(b"NOPE", 32, 32, 30, 1, 1)[..], &mut output).unwrap_err();
    match err { StreamError::Io(_) => panic!("expected legacy invalid-data classification"), StreamError::Invalid(_) => {} }
    let err = decode_y4m(&header(b"TVC0", 801, 32, 30, 1, 1)[..], &mut output).unwrap_err();
    assert!(matches!(err, StreamError::Invalid(_)));
}

#[test]
fn detailed_header_errors_use_the_offending_field_offset() {
    for (bytes, expected, is_limit) in [
        (header(b"NOPE", 32, 32, 30, 1, 1), 0, false),
        (header(b"TVC0", 0, 32, 30, 1, 1), 4, false),
        (header(b"TVC0", 32, 0, 30, 1, 1), 6, false),
        (header(b"TVC0", 32, 32, 0, 1, 1), 8, false),
        (header(b"TVC0", 32, 32, 30, 0, 1), 10, false),
        (header(b"TVC0", 32, 32, 30, 1, 0), 12, false),
        (header(b"TVC0", 801, 32, 30, 1, 1), 4, true),
        (header(b"TVC0", 32, 721, 30, 1, 1), 6, true),
        (header(b"TVC0", 32, 32, 30, 1, 432_001), 12, true),
    ] {
        let err = decode_y4m_detailed(&bytes[..], Vec::new()).unwrap_err();
        match (err, is_limit) {
            (DetailedStreamError::InvalidAt { offset, .. }, false) => assert_eq!(offset, expected),
            (DetailedStreamError::LimitExceededAt { offset, .. }, true) => assert_eq!(offset, expected),
            (other, _) => panic!("wrong error kind: {other:?}"),
        }
    }
}

#[test]
fn tvc1_level_and_reserved_bytes_follow_spec() {
    for level in [0, 4, 255] {
        let mut bytes = header(b"TVC1", 32, 32, 30, 1, 1);
        bytes.extend_from_slice(&[level, 0, 0, 0]);
        assert!(matches!(decode_y4m_detailed(&bytes[..], Vec::new()), Err(DetailedStreamError::InvalidAt { offset: 16, .. })));
    }
    let mut reserved = header(b"TVC1", 32, 32, 30, 1, 1);
    reserved.extend_from_slice(&[1, 0, 0, 1]);
    assert!(matches!(decode_y4m_detailed(&reserved[..], Vec::new()), Err(DetailedStreamError::InvalidAt { offset: 19, .. })));

    let valid_tvc1 = include_bytes!("../src/tvc/sequence-reference.tvc");
    assert_eq!(&valid_tvc1[16..20], &[1, 0, 0, 0]);
    assert!(decode_y4m_detailed(&valid_tvc1[..], Vec::new()).is_ok());
}

#[test]
fn picture_error_offset_is_payload_start_and_remains_ambiguous() {
    let mut bytes = header(b"TVC0", 32, 32, 30, 1, 1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0xff, 0, 0]);
    assert!(matches!(decode_y4m_detailed(&bytes[..], Vec::new()), Err(DetailedStreamError::AmbiguousPicture { offset: 20, .. })));
}

struct FailsAfter { remaining: usize }
impl Read for FailsAfter {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 { return Err(io::Error::new(io::ErrorKind::Other, "injected read failure")); }
        let n = self.remaining.min(out.len());
        out[..n].fill(0);
        self.remaining -= n;
        Ok(n)
    }
}
struct FailingWriter;
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::Error::new(io::ErrorKind::Other, "injected write failure")) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[test]
fn detailed_and_legacy_apis_preserve_injected_io_errors() {
    assert!(matches!(decode_y4m_detailed(FailsAfter { remaining: 8 }, Vec::new()), Err(DetailedStreamError::Io(_))));
    assert!(matches!(decode_y4m(FailsAfter { remaining: 8 }, Vec::new()), Err(StreamError::Io(_))));
    let bytes = header(b"TVC0", 32, 32, 30, 1, 1);
    assert!(matches!(decode_y4m_detailed(&bytes[..], FailingWriter), Err(DetailedStreamError::Io(_))));
    assert!(matches!(decode_y4m(&bytes[..], FailingWriter), Err(StreamError::Io(_))));
}

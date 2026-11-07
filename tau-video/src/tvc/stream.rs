//! Bounded TVC elementary-stream decoding to YUV4MPEG2.
use super::frame::{Frame420, PictureDecoder};
use std::{error, fmt, io::{self, Read, Write}};

/// TVC stream decoding failure. Kept exhaustive for source compatibility.
#[derive(Debug)]
pub enum StreamError {
    /// Input or output error.
    Io(io::Error),
    /// Invalid, unsupported, or out-of-bounds stream data.
    Invalid(&'static str),
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::Io(e) => write!(f, "I/O error: {e}"), Self::Invalid(s) => f.write_str(s) }
    }
}
impl error::Error for StreamError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self { Self::Io(e) => Some(e), Self::Invalid(_) => None }
    }
}
impl From<io::Error> for StreamError { fn from(e: io::Error) -> Self { Self::Io(e) } }

/// More precise, opt-in diagnostics for TVC decoding.
#[derive(Debug)]
#[non_exhaustive]
pub enum DetailedStreamError {
    /// Input or output error.
    Io(io::Error),
    /// Malformed syntax at a byte offset.
    InvalidAt { offset: u64, message: &'static str },
    /// Syntactically valid value outside limits supported by this decoder.
    LimitExceededAt { offset: u64, message: &'static str },
    /// Picture decoding failed; the picture decoder does not distinguish malformed from unsupported syntax.
    AmbiguousPicture { offset: u64, message: &'static str },
}
impl fmt::Display for DetailedStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::InvalidAt { message, .. } | Self::LimitExceededAt { message, .. } | Self::AmbiguousPicture { message, .. } => f.write_str(message),
        }
    }
}
impl error::Error for DetailedStreamError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> { match self { Self::Io(e) => Some(e), _ => None } }
}
impl From<io::Error> for DetailedStreamError { fn from(e: io::Error) -> Self { Self::Io(e) } }
fn emit<W: Write>(output: &mut W, frame: Frame420) -> Result<(), DetailedStreamError> {
    output.write_all(b"FRAME\n")?;
    output.write_all(&frame.y)?;
    output.write_all(&frame.cb)?;
    output.write_all(&frame.cr)?;
    Ok(())
}

/// Decode a TVC0 or TVC1 elementary stream into YUV4MPEG2 using bounded buffers.
///
/// This compatibility API retains its original exhaustive error type; malformed,
/// unsupported, and out-of-bounds input are reported as `Invalid`. TVC1
/// reserved bytes are now checked; error message text may differ.
/// Use [`decode_y4m_detailed`] for byte offsets and error classification.
pub fn decode_y4m<R: Read, W: Write>(input: R, output: W) -> Result<(), StreamError> {
    decode_y4m_detailed(input, output).map_err(|e| match e {
        DetailedStreamError::Io(e) => StreamError::Io(e),
        DetailedStreamError::InvalidAt { message, .. }
        | DetailedStreamError::LimitExceededAt { message, .. }
        | DetailedStreamError::AmbiguousPicture { message, .. } => StreamError::Invalid(message),
    })
}

/// Decode a TVC0 or TVC1 stream with detailed input classification and offsets.
///
/// Output may be partially written on error. This is not a complete TVC
/// conformance checker; level limits and all malformed payloads are not
/// established by successful decode.
///
/// Header diagnostics point to the exact field containing the invalid value.
/// Picture decoder errors point to the start of the affected picture payload;
/// the decoder cannot identify an exact failing bit within that payload.
pub fn decode_y4m_detailed<R: Read, W: Write>(mut input: R, mut output: W) -> Result<(), DetailedStreamError> {
    let mut header = [0u8; 16];
    input.read_exact(&mut header)?;
    let mut stream_offset = 16u64;
    if &header[..4] != b"TVC0" && &header[..4] != b"TVC1" {
        return Err(DetailedStreamError::InvalidAt { offset: 0, message: "Invalid TVC magic." });
    }
    let width = u16::from_le_bytes([header[4], header[5]]);
    let height = u16::from_le_bytes([header[6], header[7]]);
    let num = u16::from_le_bytes([header[8], header[9]]);
    let den = u16::from_le_bytes([header[10], header[11]]);
    let frames = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    for (bad, offset) in [(width == 0, 4), (height == 0, 6), (num == 0, 8), (den == 0, 10), (frames == 0, 12)] {
        if bad { return Err(DetailedStreamError::InvalidAt { offset, message: "Invalid TVC stream header field." }); }
    }
    for (bad, offset) in [(width > 800, 4), (height > 720, 6), (frames > crate::MAX_FRAMES, 12)] {
        if bad { return Err(DetailedStreamError::LimitExceededAt { offset, message: "TVC stream exceeds decoder limits." }); }
    }
    if &header[..4] == b"TVC1" {
        let mut level = [0u8; 4];
        input.read_exact(&mut level)?;
        stream_offset += 4;
        if !(1..=3).contains(&level[0]) {
            return Err(DetailedStreamError::InvalidAt { offset: 16, message: "Invalid TVC1 level; expected level 1, 2, or 3." });
        }
        if level[1..] != [0, 0, 0] {
            let reserved = level[1..].iter().position(|&b| b != 0).unwrap() as u64 + 17;
            return Err(DetailedStreamError::InvalidAt { offset: reserved, message: "Nonzero TVC1 reserved header byte." });
        }
    }
    let mut decoder = PictureDecoder::new(width, height).map_err(|_| DetailedStreamError::InvalidAt { offset: 4, message: "Invalid picture dimensions." })?;
    writeln!(output, "YUV4MPEG2 W{width} H{height} F{num}:{den} Ip A1:1 C420jpeg")?;
    for _ in 0..frames {
        let length_offset = stream_offset;
        let mut b = [0u8; 4];
        input.read_exact(&mut b)?;
        let len = u32::from_le_bytes(b) as usize;
        if len < 3 { return Err(DetailedStreamError::InvalidAt { offset: length_offset, message: "Invalid TVC picture size." }); }
        if len > 384_000 { return Err(DetailedStreamError::LimitExceededAt { offset: length_offset, message: "TVC picture exceeds decoder limit." }); }
        stream_offset += 4;
        let picture_offset = stream_offset;
        let mut picture = vec![0u8; len];
        input.read_exact(&mut picture)?;
        stream_offset += len as u64;
        if let Some(frame) = decoder.decode(&picture).map_err(|_| DetailedStreamError::AmbiguousPicture { offset: picture_offset, message: "Invalid or unsupported TVC picture syntax." })? { emit(&mut output, frame)?; }
    }
    if let Some(frame) = decoder.finish() { emit(&mut output, frame)?; }
    Ok(())
}

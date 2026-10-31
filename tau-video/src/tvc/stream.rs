//! Bounded TVC elementary-stream decoding to YUV4MPEG2.
use super::frame::{Frame420, PictureDecoder};
use std::{
    error, fmt,
    io::{self, Read, Write},
};
/// TVC stream decoding failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum StreamError {
    /// Input or output error.
    Io(io::Error),
    /// Invalid, unsupported or excessive stream data.
    Invalid(&'static str),
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Invalid(s) => f.write_str(s),
        }
    }
}
impl error::Error for StreamError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for StreamError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
fn emit<W: Write>(output: &mut W, frame: Frame420) -> Result<(), StreamError> {
    output.write_all(b"FRAME\n")?;
    output.write_all(&frame.y)?;
    output.write_all(&frame.cb)?;
    output.write_all(&frame.cr)?;
    Ok(())
}
/// Decode a TVC0 or TVC1 elementary stream into YUV4MPEG2 using bounded buffers.
///
/// Output is written incrementally; callers are responsible for handling partial
/// output on error. This is a decoder, not a complete TVC conformance checker.
pub fn decode_y4m<R: Read, W: Write>(mut input: R, mut output: W) -> Result<(), StreamError> {
    let mut header = [0u8; 16];
    input.read_exact(&mut header)?;
    if &header[..4] != b"TVC0" && &header[..4] != b"TVC1" {
        return Err(StreamError::Invalid("Invalid TVC magic."));
    }
    let width = u16::from_le_bytes([header[4], header[5]]);
    let height = u16::from_le_bytes([header[6], header[7]]);
    let num = u16::from_le_bytes([header[8], header[9]]);
    let den = u16::from_le_bytes([header[10], header[11]]);
    let frames = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    if width == 0
        || height == 0
        || width > 800
        || height > 720
        || num == 0
        || den == 0
        || frames == 0
        || frames > crate::MAX_FRAMES
    {
        return Err(StreamError::Invalid(
            "Invalid or excessive TVC stream header.",
        ));
    }
    if &header[..4] == b"TVC1" {
        let mut level = [0u8; 4];
        input.read_exact(&mut level)?;
        if !(1..=3).contains(&level[0]) {
            return Err(StreamError::Invalid("Invalid TVC level."));
        }
    }
    let mut decoder = PictureDecoder::new(width, height)
        .map_err(|_| StreamError::Invalid("Invalid picture dimensions."))?;
    writeln!(
        output,
        "YUV4MPEG2 W{width} H{height} F{num}:{den} Ip A1:1 C420jpeg"
    )?;
    for _ in 0..frames {
        let mut b = [0u8; 4];
        input.read_exact(&mut b)?;
        let len = u32::from_le_bytes(b) as usize;
        if !(3..=384_000).contains(&len) {
            return Err(StreamError::Invalid(
                "Invalid or excessive TVC picture size.",
            ));
        }
        let mut picture = vec![0u8; len];
        input.read_exact(&mut picture)?;
        if let Some(frame) = decoder
            .decode(&picture)
            .map_err(|_| StreamError::Invalid("Invalid TVC picture."))?
        {
            emit(&mut output, frame)?;
        }
    }
    if let Some(frame) = decoder.finish() {
        emit(&mut output, frame)?;
    }
    Ok(())
}

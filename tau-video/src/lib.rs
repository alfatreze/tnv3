//! Portable TNV3 format tools. This preliminary version supports header and index inspection.
#![forbid(unsafe_code)]

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};

/// TNV3 container version understood by this crate.
pub const TNV3_VERSION: u16 = 0;
/// Maximum number of pictures inspected; covers four hours at 30 frames per second.
pub const MAX_FRAMES: u32 = 432_000;

/// An error returned while inspecting a TNV3 stream.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Input or seek error.
    Io(io::Error),
    /// Malformed or unsupported container data, with a stable code and byte offset.
    Invalid {
        code: &'static str,
        offset: u64,
        message: &'static str,
    },
    /// A declared value exceeds the supported limit.
    LimitExceeded { offset: u64, message: &'static str },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Input/output error: {e}"),
            Self::Invalid {
                code,
                offset,
                message,
            } => write!(f, "{code} at byte {offset}: {message}"),
            Self::LimitExceeded { offset, message } => {
                write!(f, "E-TNV3-LIMIT at byte {offset}: {message}")
            }
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
fn invalid(code: &'static str, offset: u64, message: &'static str) -> Error {
    Error::Invalid {
        code,
        offset,
        message,
    }
}
fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
/// IEEE CRC-32, as used by TNV3 headers and chunks.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
/// Parsed container header information. This is not a full file-validity report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// Uncropped display width in pixels.
    pub width: u16,
    /// Uncropped display height in pixels.
    pub height: u16,
    /// Frame-rate numerator.
    pub fps_num: u32,
    /// Frame-rate denominator.
    pub fps_den: u32,
    /// TVC level.
    pub level: u8,
    /// Total video pictures.
    pub frame_count: u32,
    /// Audio channels, or zero when absent.
    pub audio_channels: u8,
    /// Audio samples per channel.
    pub audio_samples: u32,
    /// Number of TAC frames.
    pub tac_frames: u32,
    /// Header flags.
    pub flags: u16,
    /// Offset of the index.
    pub index_offset: u32,
    /// Offset of the subtitle area, or zero.
    pub subtitle_offset: u32,
    /// Length of the subtitle area.
    pub subtitle_length: u32,
    /// Total stream length in bytes.
    pub file_size: u64,
}
/// One index entry as stored in the container; chunk contents are not verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// Absolute chunk offset.
    pub offset: u32,
    /// Presentation time in 48 kHz samples.
    pub presentation_sample: u32,
    /// Index of the first TAC frame in the chunk.
    pub first_tac: u32,
    /// The raw flags and seek-frame field.
    pub flags_and_seek: u32,
}
/// A seekable container reader. Opening validates the header and index layout, not chunk contents.
pub struct Tnv3Reader<R> {
    input: R,
    info: Info,
}
impl<R: Read + Seek> Tnv3Reader<R> {
    /// Open a stream from byte zero, validating its header and declared layout.
    pub fn open(mut input: R) -> Result<Self, Error> {
        let file_size = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(0))?;
        let mut h = [0u8; 64];
        input.read_exact(&mut h)?;
        if &h[..4] != b"TNV3" {
            return Err(invalid("E-TNV3-001", 0, "Invalid magic."));
        }
        if u16le(&h, 4) != TNV3_VERSION {
            return Err(invalid("E-TNV3-002", 4, "Unsupported version."));
        }
        let flags = u16le(&h, 6);
        if flags & !7 != 0 {
            return Err(invalid("E-TNV3-003", 6, "Unknown header flags."));
        }
        if crc32(&h[..60]) != u32le(&h, 60) {
            return Err(invalid("E-TNV3-004", 60, "Header CRC mismatch."));
        }
        if u16le(&h, 22) != 0 || h[52..60].iter().any(|&x| x != 0) {
            return Err(invalid(
                "E-TNV3-005",
                22,
                "Reserved header bytes are nonzero.",
            ));
        }
        let (width, height, fps_num, fps_den, level) = (
            u16le(&h, 8),
            u16le(&h, 10),
            u32le(&h, 12),
            u32le(&h, 16),
            h[20],
        );
        if width == 0
            || height == 0
            || fps_num == 0
            || fps_num > 65535
            || fps_den == 0
            || fps_den > 65535
            || !(1..=3).contains(&level)
        {
            return Err(invalid(
                "E-TNV3-006",
                8,
                "Invalid dimensions, frame rate or level.",
            ));
        }
        let (maxw, maxh, maxmb, maxrate) = match level {
            1 => (400, 240, 375, 11250),
            2 => (800, 464, 1450, 43500),
            _ => (800, 720, 2250, 67500),
        };
        let cw = (u32::from(width) + 15) & !15;
        let ch = (u32::from(height) + 15) & !15;
        let mb = (cw / 16) * (ch / 16);
        if cw > maxw
            || ch > maxh
            || mb > maxmb
            || u64::from(mb) * u64::from(fps_num) > u64::from(maxrate as u32) * u64::from(fps_den)
            || u64::from(fps_num) > 30 * u64::from(fps_den)
        {
            return Err(invalid(
                "E-TNV3-007",
                8,
                "Declared video level limits are exceeded.",
            ));
        }
        let (channels, sample_rate, frames, samples, tac_frames) = (
            h[21],
            u32le(&h, 24),
            u32le(&h, 28),
            u32le(&h, 32),
            u32le(&h, 36),
        );
        if frames == 0 {
            return Err(invalid("E-TNV3-008", 28, "No pictures declared."));
        }
        if frames > MAX_FRAMES {
            return Err(Error::LimitExceeded {
                offset: 28,
                message: "Too many pictures.",
            });
        }
        if flags & 2 != 0 {
            if !(1..=2).contains(&channels)
                || sample_rate != 48_000
                || u64::from(tac_frames) != u64::from(samples).div_ceil(1024) + 1
            {
                return Err(invalid("E-TNV3-009", 21, "Invalid audio parameters."));
            }
        } else if channels != 0 || sample_rate != 0 || samples != 0 || tac_frames != 0 {
            return Err(invalid("E-TNV3-009", 21, "Audio fields must be zero."));
        }
        let (index_offset, subtitle_offset, subtitle_length) =
            (u32le(&h, 40), u32le(&h, 44), u32le(&h, 48));
        let index_end = u64::from(index_offset) + u64::from(frames) * 16;
        if index_offset < 64 || index_offset % 4 != 0 || index_end > file_size {
            return Err(invalid("E-TNV3-010", 40, "Invalid index layout."));
        }
        if flags & 4 != 0 {
            if u64::from(subtitle_offset) != index_end
                || u64::from(subtitle_offset) + u64::from(subtitle_length) != file_size
            {
                return Err(invalid("E-TNV3-011", 44, "Invalid subtitle layout."));
            }
        } else if subtitle_offset != 0 || subtitle_length != 0 || index_end != file_size {
            return Err(invalid(
                "E-TNV3-011",
                44,
                "Invalid trailing data or subtitle fields.",
            ));
        }
        let info = Info {
            width,
            height,
            fps_num,
            fps_den,
            level,
            frame_count: frames,
            audio_channels: channels,
            audio_samples: samples,
            tac_frames,
            flags,
            index_offset,
            subtitle_offset,
            subtitle_length,
            file_size,
        };
        Ok(Self { input, info })
    }
    /// Header information; this does not establish that chunks or subtitles are valid.
    pub fn info(&self) -> &Info {
        &self.info
    }
    /// Read an index entry without allocating an index proportional to file size.
    pub fn index_entry(&mut self, n: u32) -> Result<IndexEntry, Error> {
        if n >= self.info.frame_count {
            return Err(invalid(
                "E-TNV3-012",
                u64::from(self.info.index_offset),
                "Index entry out of range.",
            ));
        }
        self.input.seek(SeekFrom::Start(
            u64::from(self.info.index_offset) + u64::from(n) * 16,
        ))?;
        let mut b = [0; 16];
        self.input.read_exact(&mut b)?;
        Ok(IndexEntry {
            offset: u32le(&b, 0),
            presentation_sample: u32le(&b, 4),
            first_tac: u32le(&b, 8),
            flags_and_seek: u32le(&b, 12),
        })
    }
    /// Return the underlying stream.
    pub fn into_inner(self) -> R {
        self.input
    }
}

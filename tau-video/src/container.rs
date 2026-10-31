//! TNV3 container writing for a single TVC video track.
use crate::{MAX_FRAMES, crc32, tvc::picture::PictureHeader};
use std::{
    collections::VecDeque,
    error, fmt,
    io::{self, Read, Seek, SeekFrom, Write},
};
/// Container muxing failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum MuxError {
    /// Input or output error.
    Io(io::Error),
    /// Invalid, unsupported or out-of-bounds TVC input.
    Invalid(&'static str),
}
impl fmt::Display for MuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Invalid(s) => f.write_str(s),
        }
    }
}
impl error::Error for MuxError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for MuxError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
#[derive(Clone, Copy)]
struct Entry {
    offset: u32,
    flags: u32,
    display: u32,
}
/// Video-only TNV3 muxing statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MuxSummary {
    /// Number of pictures.
    pub frames: u32,
    /// Container TVC level.
    pub level: u8,
}
/// Mux a TVC1 elementary stream into a video-only TNV3 v0 container.
///
/// Both streams start at offset zero. Output is written incrementally and must
/// be discarded on error. This checks container-related TVC header, picture,
/// level and rate bounds, but is not a complete bitstream validator and does
/// not decode picture syntax. Audio and subtitles are not included.
pub fn mux_tvc<R: Read, W: Write + Seek>(mut input: R, mut out: W) -> Result<MuxSummary, MuxError> {
    if out.stream_position()? != 0 || out.seek(SeekFrom::End(0))? != 0 {
        return Err(MuxError::Invalid("Output must be an empty stream."));
    }
    let mut h = [0u8; 20];
    input.read_exact(&mut h)?;
    if &h[..4] != b"TVC1" {
        return Err(MuxError::Invalid("Only TVC1 input is supported."));
    }
    let w = u16::from_le_bytes([h[4], h[5]]);
    let height = u16::from_le_bytes([h[6], h[7]]);
    let num = u16::from_le_bytes([h[8], h[9]]);
    let den = u16::from_le_bytes([h[10], h[11]]);
    let frames = u32::from_le_bytes([h[12], h[13], h[14], h[15]]);
    let level = h[16];
    if w == 0
        || height == 0
        || num == 0
        || den == 0
        || frames == 0
        || frames > MAX_FRAMES
        || !(1..=3).contains(&level)
        || h[17..20] != [0, 0, 0]
    {
        return Err(MuxError::Invalid("Invalid TVC stream header."));
    }
    let limits = [
        (
            400u32,
            240u32,
            375u32,
            11250u32,
            96000usize,
            200000usize,
            1usize,
        ),
        (800, 464, 1450, 43500, 256000, 400000, 1),
        (800, 720, 2250, 67500, 384000, 600000, 2),
    ];
    let l = limits[usize::from(level - 1)];
    let cw = (u32::from(w) + 15) & !15;
    let ch = (u32::from(height) + 15) & !15;
    let mb = cw / 16 * (ch / 16);
    if cw > l.0
        || ch > l.1
        || mb > l.2
        || u64::from(mb) * u64::from(num) > u64::from(l.3) * u64::from(den)
        || u32::from(num) > 30 * u32::from(den)
    {
        return Err(MuxError::Invalid("TVC level limits exceeded."));
    }
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&[0; 64])?;
    let mut entries: Vec<Entry> = Vec::new();
    entries
        .try_reserve_exact(frames as usize)
        .map_err(|_| MuxError::Invalid("Index allocation failed."))?;
    let window = usize::from(num).div_ceil(usize::from(den));
    let mut sizes = VecDeque::new();
    let mut total = 0usize;
    let mut anchors = 0u32;
    let mut held: Option<usize> = None;
    let mut display = 0u32;
    for i in 0..frames {
        let mut len = [0u8; 4];
        input.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len < 3 || len > l.4 {
            return Err(MuxError::Invalid("Invalid TVC picture length."));
        }
        let mut data = vec![0; len];
        input.read_exact(&mut data)?;
        let ph = PictureHeader::parse(&data, (ch / 16) as u8)
            .map_err(|_| MuxError::Invalid("Invalid TVC picture header."))?;
        if ph.slices.len() < l.6 {
            return Err(MuxError::Invalid("Too few TVC slices for level."));
        }
        let kind = data[0];
        if (i == 0 && kind != 0) || (kind == 1 && anchors < 1) || (kind == 2 && anchors < 2) {
            return Err(MuxError::Invalid("Invalid picture reference order."));
        }
        if sizes.len() == window {
            total -= sizes
                .pop_front()
                .ok_or(MuxError::Invalid("Invalid rate window."))?;
        }
        total += len;
        if total > l.5 {
            return Err(MuxError::Invalid("TVC picture rate exceeds level."));
        }
        sizes.push_back(len);
        let offset = u32::try_from(out.stream_position()?)
            .map_err(|_| MuxError::Invalid("Container exceeds 32-bit offsets."))?;
        let flags = if kind == 0 {
            1
        } else if kind == 2 {
            2
        } else {
            0
        };
        let chunk_len =
            u32::try_from(16 + len).map_err(|_| MuxError::Invalid("Chunk length overflow."))?;
        out.write_all(&chunk_len.to_le_bytes())?;
        out.write_all(&(len as u32).to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?;
        out.write_all(&(flags as u16).to_le_bytes())?;
        out.write_all(&crc32(&data).to_le_bytes())?;
        out.write_all(&data)?;
        let padding = (4 - (chunk_len % 4)) % 4;
        out.write_all(&[0u8; 3][..padding as usize])?;
        entries.push(Entry {
            offset,
            flags,
            display: 0,
        });
        let current = entries.len() - 1;
        if kind == 2 {
            entries[current].display = display;
            display += 1;
        } else {
            if let Some(previous) = held {
                entries[previous].display = display;
                display += 1;
            }
            held = Some(current);
            anchors += 1;
        }
    }
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err(MuxError::Invalid("Trailing TVC input."));
    }
    if let Some(last) = held {
        entries[last].display = display;
    }
    let index = u32::try_from(out.stream_position()?)
        .map_err(|_| MuxError::Invalid("Container exceeds 32-bit offsets."))?;
    for e in &entries {
        let pts = u64::from(e.display) * 48000 * u64::from(den) / u64::from(num);
        let pts = u32::try_from(pts)
            .map_err(|_| MuxError::Invalid("Presentation sample exceeds format limit."))?;
        out.write_all(&e.offset.to_le_bytes())?;
        out.write_all(&pts.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(&e.flags.to_le_bytes())?;
    }
    let mut header = [0u8; 64];
    header[..4].copy_from_slice(b"TNV3");
    header[6..8].copy_from_slice(&1u16.to_le_bytes());
    header[8..10].copy_from_slice(&w.to_le_bytes());
    header[10..12].copy_from_slice(&height.to_le_bytes());
    header[12..16].copy_from_slice(&u32::from(num).to_le_bytes());
    header[16..20].copy_from_slice(&u32::from(den).to_le_bytes());
    header[20] = level;
    header[28..32].copy_from_slice(&frames.to_le_bytes());
    header[40..44].copy_from_slice(&index.to_le_bytes());
    let crc = crc32(&header[..60]);
    header[60..64].copy_from_slice(&crc.to_le_bytes());
    let end = out.stream_position()?;
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&header)?;
    out.seek(SeekFrom::Start(end))?;
    Ok(MuxSummary { frames, level })
}

/// TNV3 video-track decoding failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum VideoDecodeError {
    /// Input or output failure.
    Io(io::Error),
    /// Container header or index failure.
    Container(crate::Error),
    /// Malformed or unsupported chunk or video data.
    Invalid(&'static str),
}
impl fmt::Display for VideoDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Container(e) => write!(f, "{e}"),
            Self::Invalid(e) => f.write_str(e),
        }
    }
}
impl error::Error for VideoDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Container(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for VideoDecodeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
fn write_frame<W: Write>(
    out: &mut W,
    frame: crate::tvc::frame::Frame420,
) -> Result<(), VideoDecodeError> {
    out.write_all(b"FRAME\n")?;
    out.write_all(&frame.y)?;
    out.write_all(&frame.cb)?;
    out.write_all(&frame.cr)?;
    Ok(())
}
/// Decode the TVC video track of a TNV3 container to YUV4MPEG2.
///
/// Audio and subtitles are ignored. This verifies the header, chunk/index
/// offsets, chunk bounds and enabled chunk CRCs, but is not a full TNV3 or TVC
/// conformance checker. Output is incremental and must be discarded on error.
pub fn decode_video_y4m<R: Read + Seek, W: Write>(
    input: R,
    mut output: W,
) -> Result<(), VideoDecodeError> {
    let mut reader = crate::Tnv3Reader::open(input).map_err(VideoDecodeError::Container)?;
    let info = reader.info().clone();
    let mut decoder = crate::tvc::frame::PictureDecoder::new(info.width, info.height)
        .map_err(|_| VideoDecodeError::Invalid("Invalid video dimensions."))?;
    writeln!(
        output,
        "YUV4MPEG2 W{} H{} F{}:{} Ip A1:1 C420jpeg",
        info.width, info.height, info.fps_num, info.fps_den
    )?;
    let mut offset = 64u64;
    for n in 0..info.frame_count {
        let entry = reader.index_entry(n).map_err(VideoDecodeError::Container)?;
        if u64::from(entry.offset) != offset {
            return Err(VideoDecodeError::Invalid(
                "Chunk offset does not match index.",
            ));
        }
        reader.input.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 16];
        reader.input.read_exact(&mut head)?;
        let len = u32::from_le_bytes(
            head[..4]
                .try_into()
                .map_err(|_| VideoDecodeError::Invalid("Invalid chunk header."))?,
        ) as usize;
        let pic = u32::from_le_bytes(
            head[4..8]
                .try_into()
                .map_err(|_| VideoDecodeError::Invalid("Invalid chunk header."))?,
        ) as usize;
        let flags = u16::from_le_bytes([head[10], head[11]]);
        let checksum = u32::from_le_bytes([head[12], head[13], head[14], head[15]]);
        if !(19..=665_536).contains(&len)
            || !(3..=384_000).contains(&pic)
            || pic > len - 16
            || flags & !3 != 0
            || offset + len as u64 > u64::from(info.index_offset)
        {
            return Err(VideoDecodeError::Invalid(
                "Invalid chunk length, picture length or flags.",
            ));
        }
        let mut body = vec![0; len - 16];
        reader.input.read_exact(&mut body)?;
        if (info.flags & 1 != 0 && crc32(&body) != checksum)
            || (info.flags & 1 == 0 && checksum != 0)
        {
            return Err(VideoDecodeError::Invalid("Chunk CRC mismatch."));
        }
        let expected = match body[0] {
            0 => 1,
            1 => 0,
            2 => 2,
            _ => return Err(VideoDecodeError::Invalid("Invalid TVC picture type.")),
        };
        if flags != expected || entry.flags_and_seek & 3 != u32::from(flags) {
            return Err(VideoDecodeError::Invalid("Chunk picture flags disagree."));
        }
        if let Some(frame) = decoder
            .decode(&body[..pic])
            .map_err(|_| VideoDecodeError::Invalid("Invalid TVC picture."))?
        {
            write_frame(&mut output, frame)?;
        }
        let end = offset + len as u64;
        let padded = (end + 3) & !3;
        let padding = (padded - end) as usize;
        let mut pad = [0u8; 3];
        reader.input.read_exact(&mut pad[..padding])?;
        if pad[..padding].iter().any(|&b| b != 0) {
            return Err(VideoDecodeError::Invalid("Nonzero chunk padding."));
        }
        offset = padded;
    }
    if offset != u64::from(info.index_offset) {
        return Err(VideoDecodeError::Invalid(
            "Chunk layout does not end at index.",
        ));
    }
    if let Some(frame) = decoder.finish() {
        write_frame(&mut output, frame)?;
    }
    Ok(())
}

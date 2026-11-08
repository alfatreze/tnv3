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
    /// The finished file was rejected by this crate's own reader.
    Rejected(crate::Error),
}
impl fmt::Display for MuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Invalid(s) => f.write_str(s),
            Self::Rejected(e) => write!(f, "The muxed file was rejected: {e}"),
        }
    }
}
impl error::Error for MuxError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Rejected(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for MuxError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
/// Muxing statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MuxSummary {
    /// Number of pictures.
    pub frames: u32,
    /// Container TVC level.
    pub level: u8,
    /// TAC frames written (0 without audio).
    pub audio_frames: u32,
    /// Audio samples per channel declared in the header (after trimming).
    pub audio_samples: u32,
}

/// Options of [`mux_streams`]. `MuxOptions::default()` is a video-only file without chunk CRCs, like
/// `tnv3 mux` without flags.
#[derive(Clone, Debug, Default)]
pub struct MuxOptions<'a> {
    /// A complete TAC0 file (48 kHz); its frames are interleaved with the pictures.
    pub audio: Option<&'a [u8]>,
    /// A complete subtitle area (spec section 6.1), appended after the index.
    pub subtitles: Option<&'a [u8]>,
    /// Write a CRC-32 in every chunk header (header flag bit 0).
    pub crc: bool,
    /// TVC level to record (1..=3). Required for TVC0 input; for TVC1 it overrides the stream's level.
    pub level: Option<u8>,
    /// Frame rate override (numerator, denominator).
    pub rate: Option<(u32, u32)>,
    /// Cut audio that is longer than the video.
    pub trim_audio: bool,
    /// Audio interleave shift in TAC frames (positive: audio runs ahead).
    pub audio_shift: i32,
}

fn tac_is_reset(frame: &[u8]) -> bool {
    let mut c = 0u32;
    for i in 1..=4usize {
        c = c << 8 | u32::from(frame.get(i).copied().unwrap_or(0));
    }
    c >= 0x7FFF_FFFF
}

/// Mux a TVC1 elementary stream into a video-only TNV3 v0 container with a CRC in every chunk.
///
/// Both streams start at offset zero. Output is written once, after the result has passed this
/// crate's own container check; it must still be discarded on error. This checks container-related TVC
/// header, picture, level and rate bounds, but is not a complete bitstream validator and does not decode
/// picture syntax. For audio and subtitles use [`mux_streams`].
pub fn mux_tvc<R: Read, W: Write + Seek>(mut input: R, out: W) -> Result<MuxSummary, MuxError> {
    let mut video = Vec::new();
    input.read_to_end(&mut video)?;
    if video.len() < 4 || &video[..4] != b"TVC1" {
        return Err(MuxError::Invalid("Only TVC1 input is supported."));
    }
    mux_streams(&video, &MuxOptions { crc: true, ..MuxOptions::default() }, out)
}

/// Mux a TVC stream (TVC0 with `options.level`, or TVC1), optional TAC audio and an optional subtitle
/// area into a TNV3 v0 container; the Rust equivalent of `tnv3 mux`. Pictures keep decode order,
/// audio frames are distributed so that the audio a picture needs is in or before its chunk, and each
/// I picture records the last TAC reset frame at or before its time as its seek point. The finished file
/// is checked with [`crate::check_container_preview`] before it is written to `out`, which must be empty.
pub fn mux_streams<W: Write + Seek>(
    video: &[u8],
    options: &MuxOptions<'_>,
    mut out: W,
) -> Result<MuxSummary, MuxError> {
    if out.stream_position()? != 0 || out.seek(SeekFrom::End(0))? != 0 {
        return Err(MuxError::Invalid("Output must be an empty stream."));
    }
    // ---- video header
    let (mut p, w, height, mut num, mut den, frames, mut level);
    match video.get(..4) {
        Some(b"TVC1") if video.len() >= 20 => {
            let h = &video[..20];
            if h[17..20] != [0, 0, 0] {
                return Err(MuxError::Invalid("Invalid TVC stream header."));
            }
            (p, level) = (20usize, h[16]);
        }
        Some(b"TVC0") if video.len() >= 16 => (p, level) = (16usize, 0),
        _ => return Err(MuxError::Invalid("Not a TVC0 or TVC1 stream.")),
    }
    w = u16::from_le_bytes([video[4], video[5]]);
    height = u16::from_le_bytes([video[6], video[7]]);
    num = u32::from(u16::from_le_bytes([video[8], video[9]]));
    den = u32::from(u16::from_le_bytes([video[10], video[11]]));
    frames = u32::from_le_bytes([video[12], video[13], video[14], video[15]]);
    if let Some(l) = options.level {
        level = l;
    }
    if let Some((n, d)) = options.rate {
        (num, den) = (n, d);
    }
    if level == 0 {
        return Err(MuxError::Invalid("TVC0 input needs a level."));
    }
    if w == 0
        || height == 0
        || num == 0
        || den == 0
        || frames == 0
        || frames > MAX_FRAMES
        || !(1..=3).contains(&level)
    {
        return Err(MuxError::Invalid("Invalid TVC stream header."));
    }
    let limits = [
        (400u32, 240u32, 375u32, 11250u32, 96000usize, 200000usize, 1usize),
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
        || num > 30 * den
    {
        return Err(MuxError::Invalid("TVC level limits exceeded."));
    }
    // ---- pass 1: picture slices, validation, display order
    let mut pics: Vec<(usize, usize)> = Vec::new(); // (offset, length)
    pics.try_reserve_exact(frames as usize).map_err(|_| MuxError::Invalid("Index allocation failed."))?;
    let window = (num as usize).div_ceil(den as usize);
    let mut sizes = VecDeque::new();
    let mut total = 0usize;
    let mut anchors = 0u32;
    let mut disp: Vec<u32> = vec![0; frames as usize];
    let (mut held, mut display) = (None::<usize>, 0u32);
    for i in 0..frames as usize {
        let head = video.get(p..p + 4).ok_or(MuxError::Invalid("Truncated TVC stream."))?;
        let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
        p += 4;
        if len < 3 || len > l.4 {
            return Err(MuxError::Invalid("Invalid TVC picture length."));
        }
        let data = video.get(p..p + len).ok_or(MuxError::Invalid("Truncated TVC stream."))?;
        let ph = PictureHeader::parse(data, (ch / 16) as u8).map_err(|_| MuxError::Invalid("Invalid TVC picture header."))?;
        if ph.slices.len() < l.6 {
            return Err(MuxError::Invalid("Too few TVC slices for level."));
        }
        let kind = data[0];
        if (i == 0 && kind != 0) || (kind == 1 && anchors < 1) || (kind == 2 && anchors < 2) {
            return Err(MuxError::Invalid("Invalid picture reference order."));
        }
        if sizes.len() == window {
            total -= sizes.pop_front().ok_or(MuxError::Invalid("Invalid rate window."))?;
        }
        total += len;
        if total > l.5 {
            return Err(MuxError::Invalid("TVC picture rate exceeds level."));
        }
        sizes.push_back(len);
        pics.push((p, len));
        p += len;
        if kind == 2 {
            disp[i] = display;
            display += 1;
        } else {
            if let Some(prev) = held {
                disp[prev] = display;
                display += 1;
            }
            held = Some(i);
            anchors += 1;
        }
    }
    if p != video.len() {
        return Err(MuxError::Invalid("Trailing TVC input."));
    }
    if let Some(last) = held {
        disp[last] = display;
    }
    // ---- audio
    let sample_time = |k: u64| k * 48000 * u64::from(den) / u64::from(num);
    let mut tac: Vec<(usize, usize)> = Vec::new(); // (offset, length) of each TAC frame in the audio file
    let (mut channels, mut nsamp, mut ntac) = (0u8, 0u32, 0u32);
    if let Some(a) = options.audio {
        if a.len() < 20 || &a[..4] != b"TAC0" || u32::from_le_bytes([a[4], a[5], a[6], a[7]]) != 48000 {
            return Err(MuxError::Invalid("Not a 48 kHz TAC0 file."));
        }
        channels = a[8];
        ntac = u32::from_le_bytes([a[12], a[13], a[14], a[15]]);
        nsamp = u32::from_le_bytes([a[16], a[17], a[18], a[19]]);
        let mut q = 20usize;
        for _ in 0..ntac {
            let head = a.get(q..q + 2).ok_or(MuxError::Invalid("Truncated TAC file."))?;
            let n = usize::from(u16::from_le_bytes([head[0], head[1]]));
            a.get(q + 2..q + 2 + n).ok_or(MuxError::Invalid("Truncated TAC file."))?;
            tac.push((q + 2, n));
            q += 2 + n;
        }
        let dur = sample_time(u64::from(frames));
        if options.trim_audio && dur < u64::from(nsamp) {
            nsamp = dur as u32;
            ntac = (nsamp + 1023) / 1024 + 1;
        }
    }
    let audio = options.audio.unwrap_or(&[]);
    // ---- pass 2: chunks
    let mut o: Vec<u8> = vec![0; 64];
    let mut entries: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(frames as usize); // offset, pts, first TAC, flags|seek<<8
    let (mut max_disp, mut used) = (0u32, 0u32);
    for n in 0..frames as usize {
        max_disp = max_disp.max(disp[n]);
        let mut target = used;
        if options.audio.is_some() {
            let need = sample_time(u64::from(max_disp) + 1) / 1024 + 2;
            let tg = if n + 1 == frames as usize { i64::from(ntac) } else { need as i64 + i64::from(options.audio_shift) };
            target = tg.clamp(0, i64::from(ntac)) as u32;
            if target < used {
                target = used;
            }
        }
        let (po, pl) = pics[n];
        let pic = &video[po..po + pl];
        let kind = pic[0];
        let flags: u16 = u16::from(kind == 0) | (u16::from(kind == 2) << 1);
        let mut body = pic.to_vec();
        for i in used..target {
            let (fo, fl) = tac[i as usize];
            body.extend((fl as u16).to_le_bytes());
            body.extend(&audio[fo..fo + fl]);
        }
        let chunk_len = u32::try_from(16 + body.len()).map_err(|_| MuxError::Invalid("Chunk length overflow."))?;
        let offset = u32::try_from(o.len()).map_err(|_| MuxError::Invalid("Container exceeds 32-bit offsets."))?;
        o.extend(chunk_len.to_le_bytes());
        o.extend((pl as u32).to_le_bytes());
        o.extend(((target - used) as u16).to_le_bytes());
        o.extend(flags.to_le_bytes());
        o.extend(if options.crc { crc32(&body) } else { 0 }.to_le_bytes());
        o.extend(&body);
        while o.len() % 4 != 0 {
            o.push(0);
        }
        let sk = sample_time(u64::from(disp[n]));
        let sk32 = u32::try_from(sk).map_err(|_| MuxError::Invalid("Presentation sample exceeds format limit."))?;
        let mut seek = 0u32;
        if options.audio.is_some() && flags & 1 != 0 {
            for r in 0..ntac {
                if u64::from(r) * 1024 > u64::from(sk32) {
                    break;
                }
                let (fo, fl) = tac[r as usize];
                if tac_is_reset(&audio[fo..fo + fl]) {
                    seek = r;
                }
            }
        }
        entries.push((offset, sk32, used, u32::from(flags) | seek << 8));
        used = target;
    }
    let index = u32::try_from(o.len()).map_err(|_| MuxError::Invalid("Container exceeds 32-bit offsets."))?;
    for e in &entries {
        o.extend(e.0.to_le_bytes());
        o.extend(e.1.to_le_bytes());
        o.extend(e.2.to_le_bytes());
        o.extend(e.3.to_le_bytes());
    }
    let (mut sub_offset, mut sub_len) = (0u32, 0u32);
    if let Some(s) = options.subtitles {
        sub_offset = u32::try_from(o.len()).map_err(|_| MuxError::Invalid("Container exceeds 32-bit offsets."))?;
        sub_len = u32::try_from(s.len()).map_err(|_| MuxError::Invalid("Subtitle area too large."))?;
        o.extend(s);
    }
    // ---- header
    let flags16 = u16::from(options.crc) | if options.audio.is_some() { 2 } else { 0 } | if options.subtitles.is_some() { 4 } else { 0 };
    o[..4].copy_from_slice(b"TNV3");
    o[6..8].copy_from_slice(&flags16.to_le_bytes());
    o[8..10].copy_from_slice(&w.to_le_bytes());
    o[10..12].copy_from_slice(&height.to_le_bytes());
    o[12..16].copy_from_slice(&num.to_le_bytes());
    o[16..20].copy_from_slice(&den.to_le_bytes());
    o[20] = level;
    o[21] = channels;
    o[24..28].copy_from_slice(&(if options.audio.is_some() { 48000u32 } else { 0 }).to_le_bytes());
    o[28..32].copy_from_slice(&frames.to_le_bytes());
    o[32..36].copy_from_slice(&nsamp.to_le_bytes());
    o[36..40].copy_from_slice(&ntac.to_le_bytes());
    o[40..44].copy_from_slice(&index.to_le_bytes());
    o[44..48].copy_from_slice(&sub_offset.to_le_bytes());
    o[48..52].copy_from_slice(&sub_len.to_le_bytes());
    let crc = crc32(&o[..60]);
    o[60..64].copy_from_slice(&crc.to_le_bytes());
    // ---- self-check, then write
    let mut reader = crate::Tnv3Reader::open(io::Cursor::new(&o[..])).map_err(MuxError::Rejected)?;
    let report = crate::check_container_preview(&mut reader);
    if report.status != crate::ContainerPreviewStatus::Valid {
        let f = &report.findings[0];
        return Err(MuxError::Invalid(match f.code {
            "E-TNV3-SUB" => "The muxed file failed the subtitle structure check.",
            _ => "The muxed file failed the container check.",
        }));
    }
    out.write_all(&o)?;
    Ok(MuxSummary { frames, level, audio_frames: used, audio_samples: nsamp })
}

/// Extracts the TVC1 elementary stream (pictures in decode order) of a TNV3 container, ignoring audio and
/// subtitles; the Rust equivalent of `tnv3 demux IN OUT.tvc`. Each picture goes through
/// [`crate::Tnv3Reader::read_picture`], so chunk bounds, flags, padding and enabled CRCs are checked.
/// The frame rate must fit the 16-bit fields of the TVC1 header.
pub fn extract_tvc<R: Read + Seek>(input: R) -> Result<Vec<u8>, MuxError> {
    let mut reader = crate::Tnv3Reader::open(input).map_err(MuxError::Rejected)?;
    let info = reader.info().clone();
    let (Ok(num), Ok(den)) = (u16::try_from(info.fps_num), u16::try_from(info.fps_den)) else {
        return Err(MuxError::Invalid("Frame rate does not fit a TVC1 header."));
    };
    let mut out = Vec::new();
    out.extend(b"TVC1");
    out.extend(info.width.to_le_bytes());
    out.extend(info.height.to_le_bytes());
    out.extend(num.to_le_bytes());
    out.extend(den.to_le_bytes());
    out.extend(info.frame_count.to_le_bytes());
    out.extend([info.level, 0, 0, 0]);
    for n in 0..info.frame_count {
        let picture = reader.read_picture(n).map_err(MuxError::Rejected)?;
        out.extend((picture.len() as u32).to_le_bytes());
        out.extend(picture);
    }
    Ok(out)
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

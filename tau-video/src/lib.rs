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
    !crc32_update(!0, bytes)
}
fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc
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
#[derive(Clone, Copy)]
struct CrossFrameEntry {
    offset: u32,
    first_tac: u32,
    audio_count: u32,
    picture_len: u32,
    chunk_len: u32,
    flags: u16,
    display: u32,
}
/// A seekable container reader. Opening validates the header and index layout, not chunk contents.
pub struct Tnv3Reader<R> {
    input: R,
    info: Info,
}

/// Maximum number of per-picture validation findings retained in a report.
pub const MAX_CHUNK_STRUCTURE_FINDINGS: usize = 64;
/// Maximum subtitle area retained while validating directory-relative offsets.
/// Larger legal areas are reported as an implementation limit rather than
/// being allocated from an untrusted 32-bit length.
pub const MAX_SUBTITLE_AREA_BYTES: u32 = 64 * 1024 * 1024;

/// Summary of structurally validated subtitle data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubtitleStructureReport {
    pub tracks_checked: u8,
    pub variants_checked: u32,
    pub events_checked: u64,
    pub glyphs_checked: u64,
}

/// Status of the bounded, combined TNV3 container preview check.
/// `Valid` means only that the implemented container structure scopes passed;
/// it does not imply TVC/TAC payload decoding or full spec conformance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerPreviewStatus {
    Valid,
    Invalid,
    Indeterminate,
}

/// One fail-fast finding from a container preview check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerPreviewFinding {
    pub code: &'static str,
    pub offset: Option<u64>,
    pub message: String,
}

/// Counts and result of the combined cross-frame and subtitle structure scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerPreviewReport {
    pub status: ContainerPreviewStatus,
    /// Number of pictures only when the complete cross-frame phase passed;
    /// fail-fast errors in that phase report zero rather than partial progress.
    pub pictures_checked: u32,
    pub tracks_checked: u8,
    pub variants_checked: u32,
    pub events_checked: u64,
    pub glyphs_checked: u64,
    /// Empty for `Valid`; contains exactly one typed finding otherwise.
    pub findings: Vec<ContainerPreviewFinding>,
}

/// Run the implemented container-only preview scopes against one already-opened reader.
/// Cross-frame validation runs first and subtitles only run if it passes.
/// Input must remain stable throughout the check. I/O failures and explicit
/// resource limits yield `Indeterminate`; malformed container data yields
/// `Invalid`. This function does not open or classify a path; callers handling
/// untrusted paths must safely establish a regular file before calling it.
/// No TVC/TAC payload decoding is performed.
pub fn check_container_preview<R: Read + Seek>(
    reader: &mut Tnv3Reader<R>,
) -> ContainerPreviewReport {
    let pictures_checked = reader.info().frame_count;
    if let Err(error) = reader.check_cross_frame() {
        return container_preview_failure(error, 0);
    }
    match check_subtitle_structure(reader) {
        Ok(subtitles) => ContainerPreviewReport {
            status: ContainerPreviewStatus::Valid,
            pictures_checked,
            tracks_checked: subtitles.tracks_checked,
            variants_checked: subtitles.variants_checked,
            events_checked: subtitles.events_checked,
            glyphs_checked: subtitles.glyphs_checked,
            findings: Vec::new(),
        },
        Err(error) => container_preview_failure(error, pictures_checked),
    }
}

fn container_preview_failure(
    error: Error,
    pictures_checked: u32,
) -> ContainerPreviewReport {
    let (status, code, offset, message) = match error {
        Error::Invalid { code, offset, message } => (
            ContainerPreviewStatus::Invalid,
            code,
            Some(offset),
            message.to_string(),
        ),
        Error::LimitExceeded { offset, message } => (
            ContainerPreviewStatus::Indeterminate,
            "E-TNV3-LIMIT",
            Some(offset),
            message.to_string(),
        ),
        Error::Io(error) => (
            ContainerPreviewStatus::Indeterminate,
            "E-TNV3-IO",
            None,
            error.to_string(),
        ),
    };
    ContainerPreviewReport {
        status,
        pictures_checked,
        tracks_checked: 0,
        variants_checked: 0,
        events_checked: 0,
        glyphs_checked: 0,
        findings: vec![ContainerPreviewFinding { code, offset, message }],
    }
}

/// Validate the TNV3 subtitle area only. The caller must first construct the
/// reader with [`Tnv3Reader::open`], which checks the header and declared file
/// layout. If the header has no subtitle flag, this succeeds with an empty
/// report. Otherwise it reads at most [`MAX_SUBTITLE_AREA_BYTES`]. This does
/// not validate index entries, chunks, timing, or TVC/TAC payloads.
pub fn check_subtitle_structure<R: Read + Seek>(
    reader: &mut Tnv3Reader<R>,
) -> Result<SubtitleStructureReport, Error> {
    let (flags, subtitle_offset, subtitle_length) = {
        let info = reader.info();
        (info.flags, info.subtitle_offset, info.subtitle_length)
    };
    if flags & 4 == 0 {
        return Ok(SubtitleStructureReport::default());
    }
    if subtitle_length > MAX_SUBTITLE_AREA_BYTES {
        return Err(Error::LimitExceeded {
            offset: u64::from(subtitle_offset),
            message: "Subtitle area exceeds the 64 MiB structural-check limit.",
        });
    }
    reader
        .input
        .seek(SeekFrom::Start(u64::from(subtitle_offset)))?;
    let mut area = Vec::new();
    area.try_reserve_exact(subtitle_length as usize)
        .map_err(|_| Error::LimitExceeded {
            offset: u64::from(subtitle_offset),
            message: "Could not reserve memory for subtitle structural check.",
        })?;
    area.resize(subtitle_length as usize, 0);
    reader.input.read_exact(&mut area)?;
    validate_subtitle_area(&area, u64::from(subtitle_offset))
}

fn sub_invalid(at: u64, message: &'static str) -> Error {
    invalid("E-TNV3-SUB", at, message)
}
fn sub_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn sub_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn add_base(base: u64, at: usize) -> u64 {
    base.saturating_add(at as u64)
}
/// Checks one glyph's baked outline plane (draft 1.5): padding bits zero and
/// equal to the grey-scale dilation of the fill plane over the disc
/// `dx^2 + dy^2 <= w^2`, unclipped. `at` is the start of the fill rows.
fn check_outline_plane(
    a: &[u8],
    base: u64,
    at: usize,
    cw: usize,
    ch: usize,
    w: usize,
) -> Result<(), Error> {
    let row_bytes = (cw + 3) / 4;
    let ow = cw + 2 * w;
    let oh = ch + 2 * w;
    let o_row_bytes = (ow + 3) / 4;
    let o_at = at + ch * row_bytes;
    let fill = |x: i64, y: i64| -> u8 {
        if x < 0 || y < 0 || x >= cw as i64 || y >= ch as i64 {
            0
        } else {
            (a[at + y as usize * row_bytes + x as usize / 4] >> (2 * (x as usize & 3))) & 3
        }
    };
    let wi = w as i64;
    for oy in 0..oh {
        for ox in 0..o_row_bytes * 4 {
            let got = (a[o_at + oy * o_row_bytes + ox / 4] >> (2 * (ox & 3))) & 3;
            if ox >= ow {
                if got != 0 {
                    return Err(sub_invalid(
                        add_base(base, o_at + oy * o_row_bytes + ox / 4),
                        "Outline row padding bits must be zero.",
                    ));
                }
                continue;
            }
            let mut m = 0u8;
            for dy in -wi..=wi {
                for dx in -wi..=wi {
                    if dx * dx + dy * dy <= wi * wi {
                        m = m.max(fill(ox as i64 - wi + dx, oy as i64 - wi + dy));
                    }
                }
            }
            if m != got {
                return Err(sub_invalid(
                    add_base(base, o_at + oy * o_row_bytes + ox / 4),
                    "Outline plane differs from the dilation of the fill plane.",
                ));
            }
        }
    }
    Ok(())
}

fn align4_sub(n: usize) -> Option<usize> {
    n.checked_add(3).map(|v| v & !3)
}

fn validate_subtitle_area(a: &[u8], base: u64) -> Result<SubtitleStructureReport, Error> {
    if a.len() < 4 {
        return Err(sub_invalid(
            base,
            "Subtitle area is shorter than its header.",
        ));
    }
    let nt = usize::from(a[0]);
    if !(1..=15).contains(&nt) || a[1..4].iter().any(|&v| v != 0) {
        return Err(sub_invalid(
            base,
            "Invalid subtitle track count or reserved bytes.",
        ));
    }
    let mut dirs = Vec::with_capacity(nt);
    let mut dpos = 4usize;
    for _ in 0..nt {
        if dpos.checked_add(56).is_none_or(|end| end > a.len()) {
            return Err(sub_invalid(
                add_base(base, dpos),
                "Subtitle directory entry is truncated.",
            ));
        }
        let nv = usize::from(a[dpos + 5]);
        if !(1..=8).contains(&nv) {
            return Err(sub_invalid(
                add_base(base, dpos + 5),
                "Variant count must be 1 through 8.",
            ));
        }
        let end = dpos.checked_add(56 + 20 * nv).ok_or_else(|| {
            sub_invalid(add_base(base, dpos), "Subtitle directory size overflow.")
        })?;
        if end > a.len() {
            return Err(sub_invalid(
                add_base(base, dpos),
                "Subtitle variant directory is truncated.",
            ));
        }
        dirs.push((dpos, nv));
        dpos = end;
    }
    let mut next = align4_sub(dpos).ok_or_else(|| {
        sub_invalid(
            add_base(base, dpos),
            "Subtitle directory alignment overflow.",
        )
    })?;
    if next > a.len() || a[dpos..next].iter().any(|&v| v != 0) {
        return Err(sub_invalid(
            add_base(base, dpos),
            "Invalid directory padding.",
        ));
    }
    let mut report = SubtitleStructureReport {
        tracks_checked: nt as u8,
        ..Default::default()
    };
    let mut defaults = 0u8;
    for &(dp, nv) in &dirs {
        let lang = &a[dp..dp + 4];
        let mut ended = false;
        let mut letters = 0;
        for &c in lang {
            if c == 0 {
                ended = true;
            } else if ended || !c.is_ascii_lowercase() {
                return Err(sub_invalid(
                    add_base(base, dp),
                    "Track language must be lower-case ASCII letters followed by zero padding.",
                ));
            } else {
                letters += 1;
            }
        }
        if letters < 2 {
            return Err(sub_invalid(
                add_base(base, dp),
                "Track language must contain at least two letters.",
            ));
        }
        let flags = a[dp + 4];
        if flags & !3 != 0 || a[dp + 6] != 0 || a[dp + 7] != 0 {
            return Err(sub_invalid(
                add_base(base, dp + 4),
                "Invalid track flags or reserved field.",
            ));
        }
        defaults += flags & 1;
        let mut previous_h = 0u8;
        for v in 0..nv {
            let at = dp + 56 + 20 * v;
            let cw = a[at];
            let ch = a[at + 1];
            let outline_w = usize::from(a[at + 2]);
            if cw == 0 || ch <= previous_h || outline_w > 4 || a[at + 3] != 0 {
                return Err(sub_invalid(
                    add_base(base, at),
                    "Invalid variant cell size, order, outline width, or reserved field.",
                ));
            }
            previous_h = ch;
            let eo = sub_u32(a, at + 4) as usize;
            let el = sub_u32(a, at + 8) as usize;
            let go = sub_u32(a, at + 12) as usize;
            let gl = sub_u32(a, at + 16) as usize;
            let eend = eo
                .checked_add(el)
                .ok_or_else(|| sub_invalid(add_base(base, at + 4), "Event range overflow."))?;
            if eo != next || eend > a.len() {
                return Err(sub_invalid(
                    add_base(base, at + 4),
                    "Event list offset or bounds are invalid.",
                ));
            }
            let gstart = align4_sub(eend)
                .ok_or_else(|| sub_invalid(add_base(base, eend), "Event alignment overflow."))?;
            if gstart > a.len() || a[eend..gstart].iter().any(|&b| b != 0) || go != gstart {
                return Err(sub_invalid(
                    add_base(base, eend),
                    "Invalid padding or glyph table offset.",
                ));
            }
            let gend = go
                .checked_add(gl)
                .ok_or_else(|| sub_invalid(add_base(base, at + 16), "Glyph range overflow."))?;
            if gend > a.len() || gl < 4 {
                return Err(sub_invalid(
                    add_base(base, at + 12),
                    "Glyph table is truncated.",
                ));
            }
            let gc = usize::from(sub_u16(a, go));
            let gw = a[go + 2];
            let gh = a[go + 3];
            if gc == 0 || gc > 0xFFE0 || gw == 0 || gh == 0 {
                return Err(sub_invalid(
                    add_base(base, go),
                    "Invalid glyph table header.",
                ));
            }
            if gw != cw || gh != ch {
                return Err(sub_invalid(
                    add_base(base, go + 2),
                    "Variant cell size differs from glyph table.",
                ));
            }
            let row_bytes = (usize::from(gw) + 3) / 4;
            let fill_bytes = usize::from(gh) * row_bytes;
            let o_rows = if outline_w > 0 { usize::from(gh) + 2 * outline_w } else { 0 };
            let o_row_bytes = if outline_w > 0 {
                (usize::from(gw) + 2 * outline_w + 3) / 4
            } else {
                0
            };
            let glyph_bytes = 1usize + fill_bytes + o_rows * o_row_bytes;
            let expected = 4u64 + gc as u64 * glyph_bytes as u64;
            if expected != gl as u64 {
                return Err(sub_invalid(
                    add_base(base, at + 16),
                    "Glyph table length does not match its count and cell size.",
                ));
            }
            for i in 0..gc {
                let gp = go + 4 + i * glyph_bytes;
                if a[gp] == 0 || a[gp] > gw {
                    return Err(sub_invalid(
                        add_base(base, gp),
                        "Glyph advance is outside its cell.",
                    ));
                }
                if gw & 3 != 0 {
                    let mask = !((1u8 << (2 * (gw & 3))) - 1);
                    for row in 0..usize::from(gh) {
                        if a[gp + 1 + row * row_bytes + row_bytes - 1] & mask != 0 {
                            return Err(sub_invalid(
                                add_base(base, gp + 1 + row * row_bytes + row_bytes - 1),
                                "Glyph row padding bits must be zero.",
                            ));
                        }
                    }
                }
                if outline_w > 0 {
                    check_outline_plane(
                        a,
                        base,
                        gp + 1,
                        usize::from(gw),
                        usize::from(gh),
                        outline_w,
                    )?;
                }
            }
            if el < 4 {
                return Err(sub_invalid(
                    add_base(base, eo),
                    "Event list is shorter than its count.",
                ));
            }
            let count = sub_u32(a, eo) as usize;
            let mut p = eo + 4;
            let mut last_start = 0u32;
            for idx in 0..count {
                if p.checked_add(16).is_none_or(|end| end > eend) {
                    return Err(sub_invalid(
                        add_base(base, p),
                        "Subtitle event header is truncated.",
                    ));
                }
                let start = sub_u32(a, p);
                let end = sub_u32(a, p + 4);
                let x = sub_u16(a, p + 8);
                let y = sub_u16(a, p + 10);
                let colour = a[p + 12];
                let ef = a[p + 13];
                let nr = usize::from(sub_u16(a, p + 14));
                if start >= end {
                    return Err(sub_invalid(
                        add_base(base, p),
                        "Subtitle event end must be after start.",
                    ));
                }
                if idx > 0 && start < last_start {
                    return Err(sub_invalid(
                        add_base(base, p),
                        "Subtitle events are not sorted by start sample.",
                    ));
                }
                if x >= 800 || y >= 720 || colour > 15 || ef & !3 != 0 {
                    return Err(sub_invalid(
                        add_base(base, p + 8),
                        "Invalid subtitle event position, colour, or flags.",
                    ));
                }
                last_start = start;
                p += 16;
                let refs_end = p
                    .checked_add(nr.checked_mul(2).ok_or_else(|| {
                        sub_invalid(add_base(base, p), "Glyph reference count overflow.")
                    })?)
                    .ok_or_else(|| {
                        sub_invalid(add_base(base, p), "Glyph reference range overflow.")
                    })?;
                if refs_end > eend {
                    return Err(sub_invalid(
                        add_base(base, p),
                        "Subtitle glyph references are truncated.",
                    ));
                }
                for k in 0..nr {
                    let id = sub_u16(a, p + 2 * k);
                    if (0xFFF0..0xFFFF).contains(&id) {
                        return Err(sub_invalid(
                            add_base(base, p + 2 * k),
                            "Reserved glyph reference code.",
                        ));
                    }
                    if id < 0xFFE0 && usize::from(id) >= gc {
                        return Err(sub_invalid(
                            add_base(base, p + 2 * k),
                            "Glyph id is outside the glyph table.",
                        ));
                    }
                }
                let lines = 1 + (0..nr)
                    .filter(|&k| sub_u16(a, p + 2 * k) == 0xFFFF)
                    .count();
                if usize::from(y) + lines * usize::from(ch) > 720 {
                    return Err(sub_invalid(
                        add_base(base, p - 6),
                        "Subtitle event extends below the 720-row grid.",
                    ));
                }
                p = refs_end;
                report.events_checked += 1;
            }
            if p != eend {
                return Err(sub_invalid(
                    add_base(base, p),
                    "Event list length does not match its contents.",
                ));
            }
            let final_block = v + 1 == nv && dp == dirs.last().map_or(0, |x| x.0);
            if final_block {
                if gend != a.len() {
                    return Err(sub_invalid(
                        add_base(base, gend),
                        "Subtitle area has trailing bytes.",
                    ));
                }
                next = gend;
            } else {
                next = align4_sub(gend).ok_or_else(|| {
                    sub_invalid(add_base(base, gend), "Glyph alignment overflow.")
                })?;
                if next > a.len() || a[gend..next].iter().any(|&b| b != 0) {
                    return Err(sub_invalid(
                        add_base(base, gend),
                        "Invalid glyph table padding.",
                    ));
                }
            }
            report.variants_checked += 1;
            report.glyphs_checked += gc as u64;
        }
    }
    if defaults > 1 {
        return Err(sub_invalid(
            base,
            "More than one subtitle track is marked default.",
        ));
    }
    // Last glyph end, rather than its aligned successor, must close the area.
    Ok(report)
}

/// Result of checking indexed pictures with [`Tnv3Reader::read_picture`].
/// This checks the reader's bounded container/chunk checks and TVC picture
/// header only; it is not a full conformance or codec validation result.
#[derive(Debug, Default)]
pub struct ChunkStructureReport {
    /// Number of picture checks attempted (including failures).
    pub pictures_attempted: u32,
    /// Number of pictures whose indexed chunk passed the scoped checks.
    pub pictures_checked: u32,
    /// Retained validation failures, capped at `MAX_CHUNK_STRUCTURE_FINDINGS`.
    pub findings: Vec<ChunkStructureFinding>,
    /// Additional failures omitted after the retained findings cap.
    pub findings_omitted: u32,
}

/// A typed chunk validation failure associated with its zero-based picture index.
#[derive(Debug)]
pub struct ChunkStructureFinding {
    pub picture_index: u32,
    /// Original error, retaining its stable code and byte offset where present.
    pub error: Error,
}

/// Check every picture chunk using the process-free native reader.
///
/// This checks index-addressed chunk bounds/layout, TAC length framing,
/// enabled CRC, padding, and TVC picture header/type agreement through
/// [`Tnv3Reader::read_picture`]. It does not decode TAC or TVC payload syntax,
/// validate references/timing/subtitles, or establish full TNV3 conformance.
/// Malformed picture/chunk data is retained as a finding and scanning continues.
/// I/O errors and implementation limits stop the scan and are returned as
/// `Err(Error)`; already-collected findings are unavailable in that case.
pub fn check_chunk_structure<R: Read + Seek>(
    reader: &mut Tnv3Reader<R>,
) -> Result<ChunkStructureReport, Error> {
    let mut report = ChunkStructureReport::default();
    for picture_index in 0..reader.info().frame_count {
        report.pictures_attempted += 1;
        match reader.read_picture(picture_index) {
            Ok(_) => report.pictures_checked += 1,
            Err(error @ Error::Io(_)) | Err(error @ Error::LimitExceeded { .. }) => {
                return Err(error);
            }
            Err(error) => {
                if report.findings.len() < MAX_CHUNK_STRUCTURE_FINDINGS {
                    report.findings.push(ChunkStructureFinding {
                        picture_index,
                        error,
                    });
                } else {
                    report.findings_omitted += 1;
                }
            }
        }
    }
    Ok(report)
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
        if h[22] != 0 {
            return Err(invalid(
                "E-TNV3-032",
                22,
                "Unsupported video codec id (only 0 = TVC is defined).",
            ));
        }
        if h[23] != 0 || h[52..60].iter().any(|&x| x != 0) {
            return Err(invalid(
                "E-TNV3-005",
                23,
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
    /// Read one indexed TVC picture without decoding it.
    ///
    /// The chunk length, bounds, TAC frame lengths, flags, padding and enabled
    /// CRC are checked. TAC bytes are streamed through a fixed-size buffer;
    /// only the TVC picture is retained. The TVC picture header is parsed and
    /// its type is checked against chunk and index flags. This does not decode
    /// TAC, validate picture syntax, reference order, timing, or full container
    /// conformance. Allocation is bounded to the level's picture limit.
    pub fn read_picture(&mut self, n: u32) -> Result<Vec<u8>, Error> {
        let entry = self.index_entry(n)?;
        let offset = u64::from(entry.offset);
        if offset < 64 || offset % 4 != 0 || offset >= u64::from(self.info.index_offset) {
            return Err(invalid("E-TNV3-013", offset, "Invalid chunk offset."));
        }
        if n == 0 {
            if offset != 64 {
                return Err(invalid(
                    "E-TNV3-013",
                    offset,
                    "First chunk must start at byte 64.",
                ));
            }
        } else {
            let previous = self.index_entry(n - 1)?;
            let previous_offset = u64::from(previous.offset);
            if previous_offset < 64
                || previous_offset % 4 != 0
                || previous_offset >= offset
                || previous_offset >= u64::from(self.info.index_offset)
            {
                return Err(invalid(
                    "E-TNV3-015",
                    previous_offset,
                    "Invalid previous chunk offset.",
                ));
            }
            self.input.seek(SeekFrom::Start(previous_offset))?;
            let mut previous_len = [0u8; 4];
            self.input.read_exact(&mut previous_len)?;
            let previous_len = u32::from_le_bytes(previous_len) as u64;
            let previous_end = previous_offset
                .checked_add(previous_len)
                .and_then(|x| x.checked_add(3))
                .map(|x| x & !3)
                .ok_or_else(|| {
                    invalid(
                        "E-TNV3-015",
                        previous_offset,
                        "Previous chunk length overflow.",
                    )
                })?;
            if previous_len < 19 || previous_end != offset {
                return Err(invalid(
                    "E-TNV3-015",
                    offset,
                    "Chunk overlaps or leaves a gap after previous chunk.",
                ));
            }
        }
        self.input.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 16];
        self.input.read_exact(&mut head)?;
        let chunk_len = u32le(&head, 0) as u64;
        let picture_len = u32le(&head, 4) as usize;
        let audio_count = u16le(&head, 8);
        let flags = u16le(&head, 10);
        let checksum = u32le(&head, 12);
        let (picture_limit, chunk_limit) = match self.info.level {
            1 => (96_000, 265_536),
            2 => (256_000, 465_536),
            _ => (384_000, 665_536),
        };
        if flags & !3 != 0
            || !(3..=picture_limit).contains(&picture_len)
            || chunk_len > chunk_limit
            || (self.info.flags & 2 == 0 && audio_count != 0)
            || u64::from(entry.first_tac) + u64::from(audio_count) > u64::from(self.info.tac_frames)
        {
            return Err(invalid("E-TNV3-014", offset, "Invalid chunk header."));
        }
        let body_len = chunk_len
            .checked_sub(16)
            .ok_or_else(|| invalid("E-TNV3-014", offset, "Invalid chunk length."))?;
        let raw_end = offset
            .checked_add(chunk_len)
            .ok_or_else(|| invalid("E-TNV3-014", offset, "Chunk length overflow."))?;
        let padded_end = raw_end
            .checked_add(3)
            .map(|x| x & !3)
            .ok_or_else(|| invalid("E-TNV3-014", offset, "Chunk length overflow."))?;
        if chunk_len < 19
            || picture_len as u64 > body_len
            || padded_end > u64::from(self.info.index_offset)
        {
            return Err(invalid("E-TNV3-014", offset, "Chunk exceeds video area."));
        }
        // Adjacent index offsets must describe a contiguous chunk layout. This
        // prevents a crafted entry from overlapping its neighbor.
        let expected_end = if n + 1 < self.info.frame_count {
            u64::from(self.index_entry(n + 1)?.offset)
        } else {
            u64::from(self.info.index_offset)
        };
        if padded_end != expected_end {
            return Err(invalid(
                "E-TNV3-015",
                offset,
                "Chunk layout disagrees with index.",
            ));
        }
        self.input.seek(SeekFrom::Start(offset + 16))?;
        let mut picture = vec![0; picture_len];
        self.input.read_exact(&mut picture)?;
        let mut crc = crc32_update(!0, &picture);
        let mut consumed = picture_len as u64;
        let mut buffer = [0u8; 4096];
        for _ in 0..audio_count {
            if body_len.saturating_sub(consumed) < 2 {
                return Err(invalid(
                    "E-TNV3-020",
                    offset + 16 + consumed,
                    "Missing TAC frame length.",
                ));
            }
            let mut length_bytes = [0u8; 2];
            self.input.read_exact(&mut length_bytes)?;
            crc = crc32_update(crc, &length_bytes);
            consumed += 2;
            let frame_len = u16::from_le_bytes(length_bytes) as usize;
            if frame_len == 0 || consumed + frame_len as u64 > body_len {
                return Err(invalid(
                    "E-TNV3-020",
                    offset + 16 + consumed - 2,
                    "Invalid TAC frame length.",
                ));
            }
            let mut remaining = frame_len;
            while remaining > 0 {
                let take = remaining.min(buffer.len());
                self.input.read_exact(&mut buffer[..take])?;
                crc = crc32_update(crc, &buffer[..take]);
                remaining -= take;
            }
            consumed += frame_len as u64;
        }
        if consumed != body_len {
            return Err(invalid(
                "E-TNV3-020",
                offset + 16 + consumed,
                "Chunk body has unaccounted bytes.",
            ));
        }
        if (self.info.flags & 1 != 0 && !crc != checksum)
            || (self.info.flags & 1 == 0 && checksum != 0)
        {
            return Err(invalid("E-TNV3-016", offset + 12, "Chunk CRC mismatch."));
        }
        let mut pad = [0u8; 3];
        let padding = (padded_end - raw_end) as usize;
        self.input.read_exact(&mut pad[..padding])?;
        if pad[..padding].iter().any(|&b| b != 0) {
            return Err(invalid("E-TNV3-017", raw_end, "Nonzero chunk padding."));
        }
        let mb_rows = (((u32::from(self.info.height) + 15) & !15) / 16) as u8;
        crate::tvc::picture::PictureHeader::parse(&picture, mb_rows)
            .map_err(|_| invalid("E-TNV3-018", offset + 16, "Invalid TVC picture header."))?;
        let expected_flags = match picture[0] {
            0 => 1,
            1 => 0,
            2 => 2,
            _ => {
                return Err(invalid(
                    "E-TNV3-018",
                    offset + 16,
                    "Invalid TVC picture type.",
                ));
            }
        };
        if flags != expected_flags || entry.flags_and_seek & 3 != u32::from(flags) {
            return Err(invalid(
                "E-TNV3-019",
                offset + 10,
                "Picture flags disagree.",
            ));
        }
        Ok(picture)
    }
    /// Check a bounded cross-frame subset of TNV3 structure and index rules.
    ///
    /// Every picture is passed through [`Self::read_picture`], which checks its
    /// level size, TVC picture header, chunk framing, padding, and enabled CRC.
    /// This method then checks reference-anchor order, display-order timestamps,
    /// cumulative TAC positions, key-frame reset seek points, interleaving, and
    /// rolling picture and chunk byte-rate windows. It does not validate TAC
    /// syntax, subtitles, or full TVC entropy/reference conformance.
    /// The input must remain stable for the duration of the check.
    pub fn check_cross_frame(&mut self) -> Result<(), Error> {
        let info = self.info.clone();
        let mut entries = Vec::<CrossFrameEntry>::new();
        entries
            .try_reserve_exact(info.frame_count as usize)
            .map_err(|_| Error::LimitExceeded {
                offset: 28,
                message: "Cross-frame metadata allocation failed.",
            })?;
        let mut tac_count = 0u32;
        let mut anchors = 0u32;
        let mut held: Option<usize> = None;
        let mut display = 0u32;

        for n in 0..info.frame_count {
            let index = self.index_entry(n)?;
            let chunk_offset = u64::from(index.offset);
            let header_end = chunk_offset.checked_add(16).ok_or_else(|| {
                invalid("E-TNV3-027", chunk_offset, "Chunk header offset overflow.")
            })?;
            if chunk_offset < 64
                || chunk_offset % 4 != 0
                || header_end > u64::from(info.index_offset)
                || header_end > info.file_size
            {
                return Err(invalid(
                    "E-TNV3-027",
                    chunk_offset,
                    "Indexed chunk header is outside the chunk area.",
                ));
            }
            // Read only the fixed chunk header before accepting its advertised
            // TAC count. This prevents scanning a frame that would exceed the
            // declared total, even if a forged index starts at an earlier frame.
            self.input.seek(SeekFrom::Start(chunk_offset))?;
            let mut head = [0u8; 16];
            self.input.read_exact(&mut head)?;
            let audio_count = u32::from(u16le(&head, 8));
            let cumulative = tac_count.checked_add(audio_count).ok_or_else(|| {
                invalid(
                    "E-TNV3-020",
                    u64::from(index.offset),
                    "TAC frame count overflow.",
                )
            })?;
            if index.first_tac != tac_count || cumulative > info.tac_frames {
                return Err(invalid(
                    "E-TNV3-020",
                    u64::from(info.index_offset) + u64::from(n) * 16 + 8,
                    "Cumulative TAC positions exceed or disagree with the header.",
                ));
            }

            let picture = self.read_picture(n)?;
            let kind = picture[0];
            let flags = u16le(&head, 10);
            if (n == 0 && kind != 0) || (kind == 1 && anchors == 0) || (kind == 2 && anchors < 2) {
                return Err(invalid(
                    "E-TNV3-021",
                    u64::from(index.offset) + 16,
                    "Picture appears before its required reference anchor.",
                ));
            }
            let current = entries.len();
            entries.push(CrossFrameEntry {
                offset: index.offset,
                first_tac: tac_count,
                audio_count,
                picture_len: u32le(&head, 4),
                chunk_len: u32le(&head, 0),
                flags,
                display: 0,
            });
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
            tac_count = cumulative;
        }
        if let Some(last) = held {
            entries[last].display = display;
            display += 1;
        }
        if display != info.frame_count || tac_count != info.tac_frames {
            return Err(invalid(
                "E-TNV3-022",
                u64::from(info.index_offset),
                "Chunk totals disagree with the header.",
            ));
        }

        let mut key_targets = Vec::<(u64, u32)>::new();
        key_targets.try_reserve_exact(entries.len()).map_err(|_| {
            Error::LimitExceeded { offset: 28, message: "Seek validation allocation failed." }
        })?;
        let window = info.fps_num.div_ceil(info.fps_den) as usize;
        let (picture_window_limit, chunk_window_limit) = match info.level {
            1 => (200_000u64, 265_536u64),
            2 => (400_000, 465_536),
            _ => (600_000, 665_536),
        };
        // The validated level/frame-rate caps bound this to 1..=30 entries.
        let mut picture_ring = [0u64; 30];
        let mut chunk_ring = [0u64; 30];
        let (mut picture_sum, mut chunk_sum, mut max_display) = (0u64, 0u64, 0u32);
        for (n, chunk) in entries.iter().enumerate() {
            let entry = self.index_entry(n as u32)?;
            let sample = u64::from(chunk.display) * 48_000 * u64::from(info.fps_den)
                / u64::from(info.fps_num);
            if sample > u64::from(u32::MAX) {
                return Err(invalid(
                    "E-TNV3-023",
                    u64::from(info.index_offset) + n as u64 * 16 + 4,
                    "Presentation sample exceeds 32 bits.",
                ));
            }
            if entry.offset != chunk.offset
                || entry.presentation_sample != sample as u32
                || entry.first_tac != chunk.first_tac
                || entry.flags_and_seek & 0xfc != 0
                || entry.flags_and_seek & 3 != u32::from(chunk.flags)
            {
                return Err(invalid(
                    "E-TNV3-024",
                    u64::from(info.index_offset) + n as u64 * 16,
                    "Index entry disagrees with chunk order or display timing.",
                ));
            }
            let slot = n % window;
            if n >= window {
                picture_sum -= picture_ring[slot];
                chunk_sum -= chunk_ring[slot];
            }
            picture_ring[slot] = u64::from(chunk.picture_len);
            chunk_ring[slot] = u64::from(chunk.chunk_len);
            picture_sum += picture_ring[slot];
            chunk_sum += chunk_ring[slot];
            if picture_sum > picture_window_limit {
                return Err(invalid(
                    "E-TNV3-028",
                    u64::from(chunk.offset) + 4,
                    "Picture bytes exceed the rolling level window.",
                ));
            }
            if chunk_sum > chunk_window_limit {
                return Err(invalid(
                    "E-TNV3-029",
                    u64::from(chunk.offset),
                    "Chunk bytes exceed the rolling level window.",
                ));
            }
            max_display = max_display.max(chunk.display);
            if info.flags & 2 != 0 {
                let next_sample = u64::from(max_display + 1) * 48_000
                    * u64::from(info.fps_den)
                    / u64::from(info.fps_num);
                let need = (next_sample / 1024 + 2).min(u64::from(info.tac_frames));
                let stored = u64::from(chunk.first_tac) + u64::from(chunk.audio_count);
                if stored < need {
                    return Err(invalid(
                        "E-TNV3-030",
                        u64::from(chunk.offset) + 8,
                        "Audio interleaving lower bound is not met.",
                    ));
                }
                if n + 1 < entries.len() && stored > need + 4 {
                    return Err(invalid(
                        "E-TNV3-031",
                        u64::from(chunk.offset) + 8,
                        "Audio interleaving upper bound is exceeded.",
                    ));
                }
            }
            if chunk.flags & 1 != 0 {
                key_targets.push((sample, n as u32));
            } else if entry.flags_and_seek >> 8 != 0 {
                return Err(invalid(
                    "E-TNV3-024",
                    u64::from(info.index_offset) + n as u64 * 16 + 12,
                    "Non-key chunk has a TAC seek point.",
                ));
            }
        }
        key_targets.sort_unstable_by_key(|&(sample, _)| sample);

        // Stream reset bits in TAC file order. Retain only the latest reset
        // index, not a vector proportional to the untrusted TAC frame count.
        let mut scan_chunk = 0usize;
        let mut scan_local = 0u32;
        let mut scan_at = 0u64;
        let mut scanned = 0u32;
        let mut latest_reset = 0u32;
        let mut frame_zero_reset = false;
        for (target, n) in key_targets {
            let max_frame = target / 1024;
            while scanned < info.tac_frames && u64::from(scanned) <= max_frame {
                while scan_chunk < entries.len() && scan_local >= entries[scan_chunk].audio_count {
                    scan_chunk += 1;
                    scan_local = 0;
                    if scan_chunk < entries.len() {
                        scan_at = u64::from(entries[scan_chunk].offset)
                            + 16
                            + u64::from(entries[scan_chunk].picture_len);
                    }
                }
                if scan_chunk >= entries.len() {
                    return Err(invalid("E-TNV3-025", 0, "TAC reset scan ended early."));
                }
                if scanned == 0 {
                    scan_at = u64::from(entries[scan_chunk].offset)
                        + 16
                        + u64::from(entries[scan_chunk].picture_len);
                }
                self.input.seek(SeekFrom::Start(scan_at))?;
                let mut len_bytes = [0u8; 2];
                self.input.read_exact(&mut len_bytes)?;
                let frame_len = usize::from(u16::from_le_bytes(len_bytes));
                let mut prefix = [0u8; 5];
                let take = frame_len.min(prefix.len());
                self.input.read_exact(&mut prefix[..take])?;
                let code = (u32::from(prefix[1]) << 24)
                    | (u32::from(prefix[2]) << 16)
                    | (u32::from(prefix[3]) << 8)
                    | u32::from(prefix[4]);
                let reset = code >= 0x7fff_ffff;
                if scanned == 0 {
                    frame_zero_reset = reset;
                }
                if reset {
                    latest_reset = scanned;
                }
                scan_at += 2 + frame_len as u64;
                scan_local += 1;
                scanned += 1;
            }
            if info.flags & 2 != 0 && !frame_zero_reset {
                return Err(invalid("E-TNV3-025", 0, "First TAC frame is not a reset."));
            }
            let entry = self.index_entry(n)?;
            let expected_seek = if info.flags & 2 != 0 { latest_reset } else { 0 };
            if entry.flags_and_seek >> 8 != expected_seek {
                return Err(invalid(
                    "E-TNV3-026",
                    u64::from(info.index_offset) + u64::from(n) * 16 + 12,
                    "Key-frame seek_tac does not match the latest eligible reset.",
                ));
            }
        }
        Ok(())
    }
    /// Return the underlying stream.
    pub fn into_inner(self) -> R {
        self.input
    }
}

/// TVC video codec building blocks.
pub mod tvc;

/// TNV3 container tools.
pub mod container;

/// Optional safe adapter to the external reference C encoder workflow.
#[cfg(feature = "c-encoder")]
pub mod c_encoder;
#[cfg(feature = "c-audio-decoder")]
pub mod c_audio;
#[cfg(feature = "c-mux")]
pub mod c_mux;
#[cfg(feature = "c-subtitles")]
pub mod c_subtitles;
pub mod subtitle_read;
#[cfg(feature = "subtitle-builder")]
pub mod subtitle;

/// Experimental Y4M-to-C segment orchestration, disabled by default.
#[cfg(feature = "experimental-c-orchestrator")]
pub mod experimental_c_orchestrator;
#[cfg(feature = "experimental-c-orchestrator")]
pub mod source_gate;

/// Optional streaming reader for grouping exact 8-bit 4:2:0 Y4M frames.
pub mod y4m;

#[cfg(test)]
mod subtitle_structure_tests {
    use super::*;
    fn put16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn valid_area() -> Vec<u8> {
        // One English track, one 2x1 cell variant, one event and one glyph.
        let mut a = vec![0; 110];
        a[0] = 1;
        a[4..7].copy_from_slice(b"eng");
        a[9] = 1; // variant count at entry byte 5
        a[60] = 2;
        a[61] = 1;
        put32(&mut a, 64, 80);
        put32(&mut a, 68, 22);
        put32(&mut a, 72, 104);
        put32(&mut a, 76, 6);
        put32(&mut a, 80, 1);
        put32(&mut a, 84, 10);
        put32(&mut a, 88, 20);
        put16(&mut a, 92, 20);
        put16(&mut a, 94, 30);
        a[96] = 0;
        a[97] = 0;
        put16(&mut a, 98, 1);
        put16(&mut a, 100, 0);
        put16(&mut a, 104, 1);
        a[106] = 2;
        a[107] = 1;
        a[108] = 2;
        a[109] = 0x0f;
        a
    }
    #[test]
    fn accepts_minimal_synthetic_subtitle_area() {
        let report = validate_subtitle_area(&valid_area(), 0x1000).unwrap();
        assert_eq!(
            report,
            SubtitleStructureReport {
                tracks_checked: 1,
                variants_checked: 1,
                events_checked: 1,
                glyphs_checked: 1
            }
        );
    }
    #[test]
    fn rejects_truncated_event_and_reserved_reference() {
        let mut a = valid_area();
        put16(&mut a, 98, 2);
        assert!(matches!(
            validate_subtitle_area(&a, 0),
            Err(Error::Invalid {
                message: "Subtitle glyph references are truncated.",
                ..
            })
        ));
        let mut a = valid_area();
        put16(&mut a, 100, 0xfff0);
        assert!(matches!(
            validate_subtitle_area(&a, 0),
            Err(Error::Invalid {
                message: "Reserved glyph reference code.",
                ..
            })
        ));
    }
    #[test]
    fn rejects_bad_padding_and_glyph_advance() {
        let mut a = valid_area();
        a[102] = 1;
        assert!(validate_subtitle_area(&a, 0).is_err());
        let mut a = valid_area();
        a[108] = 0;
        assert!(matches!(
            validate_subtitle_area(&a, 0),
            Err(Error::Invalid {
                message: "Glyph advance is outside its cell.",
                ..
            })
        ));
    }
    #[test]
    fn rejects_mutated_directory_offset() {
        let mut a = valid_area();
        put32(&mut a, 64, 84);
        assert!(matches!(
            validate_subtitle_area(&a, 0),
            Err(Error::Invalid {
                message: "Event list offset or bounds are invalid.",
                ..
            })
        ));
    }
    #[test]
    fn oversized_area_stops_before_read_or_allocation() {
        struct Sparse {
            pos: u64,
            end: u64,
            prefix: [u8; 80],
        }
        impl Read for Sparse {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.pos >= self.end {
                    return Ok(0);
                }
                let n = (out.len() as u64).min(self.end - self.pos) as usize;
                for (i, b) in out[..n].iter_mut().enumerate() {
                    *b = self.prefix.get(self.pos as usize + i).copied().unwrap_or(0);
                }
                self.pos += n as u64;
                Ok(n)
            }
        }
        impl Seek for Sparse {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                let next = match from {
                    SeekFrom::Start(n) => n,
                    SeekFrom::End(n) => (self.end as i128 + n as i128) as u64,
                    SeekFrom::Current(n) => (self.pos as i128 + n as i128) as u64,
                };
                self.pos = next;
                Ok(next)
            }
        }
        let length = MAX_SUBTITLE_AREA_BYTES + 1;
        let mut h = [0u8; 80];
        h[..4].copy_from_slice(b"TNV3");
        put16(&mut h, 4, 0);
        put16(&mut h, 6, 4);
        put16(&mut h, 8, 16);
        put16(&mut h, 10, 16);
        put32(&mut h, 12, 1);
        put32(&mut h, 16, 1);
        h[20] = 1;
        put32(&mut h, 28, 1);
        put32(&mut h, 40, 64);
        put32(&mut h, 44, 80);
        put32(&mut h, 48, length);
        let crc = crc32(&h[..60]);
        put32(&mut h, 60, crc);
        let input = Sparse {
            pos: 0,
            end: 80 + u64::from(length),
            prefix: h,
        };
        let mut reader = Tnv3Reader::open(input).unwrap();
        assert!(matches!(
            check_subtitle_structure(&mut reader),
            Err(Error::LimitExceeded { offset: 80, .. })
        ));
    }
}

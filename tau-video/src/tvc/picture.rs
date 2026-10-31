//! Parsing of TVC draft 5 picture headers and slice tables.
use std::{error, fmt, ops::Range};

/// TVC picture kind in decoding order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureType {
    /// Intra-coded anchor.
    Intra,
    /// Predictive anchor.
    Predictive,
    /// Non-reference bidirectional picture.
    Bidirectional,
}
/// Invalid TVC picture header or slice layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureError(&'static str);
impl fmt::Display for PictureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl error::Error for PictureError {}
/// Parsed picture header and byte ranges of its independent entropy-coded slices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureHeader {
    /// Picture kind.
    pub kind: PictureType,
    /// Picture quantizer (0 through 51).
    pub qp: u8,
    /// Adaptive macroblock quantization flag.
    pub adaptive_quantization: bool,
    /// Quarter-pixel motion vector flag.
    pub quarter_pixel: bool,
    /// 4x4 luma transforms allowed.
    pub transform_4x4: bool,
    /// Macroblock rows per slice, or None for an unsliced picture.
    pub rows_per_slice: Option<u8>,
    /// Slice byte ranges relative to the start of this picture.
    pub slices: Vec<Range<usize>>,
}
impl PictureHeader {
    /// Parse a picture for a coded picture height in macroblock rows (1..=45).
    /// This checks header and slice boundaries, not entropy-coded content or references.
    pub fn parse(data: &[u8], mb_rows: u8) -> Result<Self, PictureError> {
        if mb_rows == 0 || mb_rows > 45 {
            return Err(PictureError("Invalid macroblock height."));
        }
        if data.len() < 3 {
            return Err(PictureError("Truncated picture header."));
        }
        let kind = match data[0] {
            0 => PictureType::Intra,
            1 => PictureType::Predictive,
            2 => PictureType::Bidirectional,
            _ => return Err(PictureError("Invalid picture type.")),
        };
        let qp = data[1];
        let flags = data[2];
        if qp > 51 || flags & 0xf0 != 0 {
            return Err(PictureError("Invalid quantizer or reserved flags."));
        }
        let sliced = flags & 8 != 0;
        let (rows, mut pos) = if sliced {
            let r = *data.get(3).ok_or(PictureError("Truncated slice header."))?;
            if r == 0 {
                return Err(PictureError("Slice row count is zero."));
            }
            (r, 4usize)
        } else {
            (mb_rows, 3usize)
        };
        let count = usize::from(mb_rows).div_ceil(usize::from(rows));
        let table_bytes = (count - 1) * 4;
        let table_end = pos
            .checked_add(table_bytes)
            .ok_or(PictureError("Slice table overflow."))?;
        if table_end > data.len() {
            return Err(PictureError("Truncated slice table."));
        }
        let mut lengths = Vec::with_capacity(count.saturating_sub(1));
        while pos < table_end {
            let b = &data[pos..pos + 4];
            lengths.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
            pos += 4;
        }
        let mut slices = Vec::with_capacity(count);
        for len in lengths {
            let end = pos
                .checked_add(len)
                .ok_or(PictureError("Slice length overflow."))?;
            if end > data.len() {
                return Err(PictureError("Slice exceeds picture."));
            }
            slices.push(pos..end);
            pos = end;
        }
        slices.push(pos..data.len());
        Ok(Self {
            kind,
            qp,
            adaptive_quantization: flags & 1 != 0,
            quarter_pixel: flags & 2 != 0,
            transform_4x4: flags & 4 != 0,
            rows_per_slice: if sliced { Some(rows) } else { None },
            slices,
        })
    }
}

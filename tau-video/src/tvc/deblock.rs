//! TVC draft 5 picture deblocking, applied after reconstruction.
use super::transform::QS;
use std::{error, fmt};
/// Macroblock data needed for determining deblocking boundary strength.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeblockMacroblock {
    /// True for an intra-coded macroblock.
    pub intra: bool,
    /// Bit mask of used reference lists (bits 0 and 1).
    pub uses: u8,
    /// Motion vectors by reference list, in the picture's motion-vector units.
    pub motion: [[i16; 2]; 2],
    /// Six-bit coded-block mask.
    pub coded: u8,
    /// Quantizer from 0 to 51.
    pub qp: u8,
}
/// Invalid frame dimensions, plane lengths or macroblock metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDeblockInput;
impl fmt::Display for InvalidDeblockInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC deblocking input.")
    }
}
impl error::Error for InvalidDeblockInput {}
fn differs(a: &DeblockMacroblock, b: &DeblockMacroblock, qpel: bool) -> bool {
    if a.uses != b.uses {
        return true;
    }
    let threshold = if qpel { 4 } else { 2 };
    (0..2).any(|i| {
        a.uses & (1 << i) != 0
            && (0..2)
                .any(|j| (i32::from(a.motion[i][j]) - i32::from(b.motion[i][j])).abs() >= threshold)
    })
}
fn luma_strength(
    a: &DeblockMacroblock,
    b: &DeblockMacroblock,
    ab: usize,
    bb: usize,
    same: bool,
    qpel: bool,
) -> u8 {
    if a.intra || b.intra {
        2
    } else if (a.coded >> ab) & 1 != 0 || (b.coded >> bb) & 1 != 0 || (!same && differs(a, b, qpel))
    {
        1
    } else {
        0
    }
}
fn chroma_strength(a: &DeblockMacroblock, b: &DeblockMacroblock, qpel: bool) -> u8 {
    if a.intra || b.intra {
        2
    } else if a.coded != 0 || b.coded != 0 || differs(a, b, qpel) {
        1
    } else {
        0
    }
}
fn filter(plane: &mut [u8], idx: [usize; 4], bs: u8, qp: u8) {
    if bs == 0 {
        return;
    }
    let q = QS[usize::from(qp)] as i32;
    let alpha = ((5 * q) >> 5) + 1;
    let beta = (q >> 4) + 1;
    let tc = ((i32::from(bs) * q) >> 6).max(1);
    let [p1, p0, q0, q1] = idx.map(|i| i32::from(plane[i]));
    if (p0 - q0).abs() >= alpha || (p1 - p0).abs() >= beta || (q1 - q0).abs() >= beta {
        return;
    }
    let d = (((q0 - p0) * 4 + (p1 - q1) + 4) >> 3).clamp(-tc, tc);
    plane[idx[1]] = (p0 + d).clamp(0, 255) as u8;
    plane[idx[2]] = (q0 - d).clamp(0, 255) as u8;
}
/// Deblock a coded-size YUV 4:2:0 picture in place.
///
/// Planes are tightly packed and have at least the declared coded size. The
/// operation checks all metadata before writing. The dimensions must be
/// multiples of 16 and fit TVC level 3; macroblocks are in raster order.
pub fn deblock(
    y: &mut [u8],
    cb: &mut [u8],
    cr: &mut [u8],
    width: usize,
    height: usize,
    mb: &[DeblockMacroblock],
    quarter_pixel: bool,
) -> Result<(), InvalidDeblockInput> {
    if width == 0
        || height == 0
        || width > 800
        || height > 720
        || width % 16 != 0
        || height % 16 != 0
    {
        return Err(InvalidDeblockInput);
    }
    let (mw, mh) = (width / 16, height / 16);
    if y.len() < width * height
        || cb.len() < width * height / 4
        || cr.len() < width * height / 4
        || mb.len() != mw * mh
        || mb.iter().any(|m| m.qp > 51 || m.uses > 3 || m.coded > 63)
    {
        return Err(InvalidDeblockInput);
    }
    for x in (8..width).step_by(8) {
        for row in 0..height {
            let ai = (row / 16) * mw + (x - 8) / 16;
            let bi = (row / 16) * mw + x / 16;
            let a = &mb[ai];
            let b = &mb[bi];
            let ab = ((row / 8) & 1) * 2 + (((x / 8) - 1) & 1);
            let bb = ((row / 8) & 1) * 2 + ((x / 8) & 1);
            let bs = luma_strength(a, b, ab, bb, ai == bi, quarter_pixel);
            let qp = ((u16::from(a.qp) + u16::from(b.qp) + 1) >> 1) as u8;
            let p = row * width + x;
            filter(y, [p - 2, p - 1, p, p + 1], bs, qp);
        }
    }
    for row in (8..height).step_by(8) {
        for x in 0..width {
            let ai = ((row - 8) / 16) * mw + x / 16;
            let bi = (row / 16) * mw + x / 16;
            let a = &mb[ai];
            let b = &mb[bi];
            let ab = ((((row / 8) - 1) & 1) * 2) + ((x / 8) & 1);
            let bb = (((row / 8) & 1) * 2) + ((x / 8) & 1);
            let bs = luma_strength(a, b, ab, bb, ai == bi, quarter_pixel);
            let qp = ((u16::from(a.qp) + u16::from(b.qp) + 1) >> 1) as u8;
            let p = row * width + x;
            filter(y, [p - 2 * width, p - width, p, p + width], bs, qp);
        }
    }
    let (cw, ch) = (width / 2, height / 2);
    for plane in [cb, cr] {
        for x in (8..cw).step_by(8) {
            for row in 0..ch {
                let a = &mb[(row / 8) * mw + x / 8 - 1];
                let b = &mb[(row / 8) * mw + x / 8];
                let bs = chroma_strength(a, b, quarter_pixel);
                let qp = ((u16::from(a.qp) + u16::from(b.qp) + 1) >> 1) as u8;
                let p = row * cw + x;
                filter(plane, [p - 2, p - 1, p, p + 1], bs, qp);
            }
        }
        for row in (8..ch).step_by(8) {
            for x in 0..cw {
                let a = &mb[(row / 8 - 1) * mw + x / 8];
                let b = &mb[(row / 8) * mw + x / 8];
                let bs = chroma_strength(a, b, quarter_pixel);
                let qp = ((u16::from(a.qp) + u16::from(b.qp) + 1) >> 1) as u8;
                let p = row * cw + x;
                filter(plane, [p - 2 * cw, p - cw, p, p + cw], bs, qp);
            }
        }
    }
    Ok(())
}

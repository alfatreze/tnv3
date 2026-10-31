//! Reconstruction of standalone TVC intra pictures.
use super::{
    deblock::{DeblockMacroblock, deblock},
    intra::{IntraMacroblock, IntraSliceDecoder},
    picture::{PictureHeader, PictureType},
    predict::{IntraMode, intra_8x8},
    transform::{inverse_4x4, inverse_8x8},
};
use std::{error, fmt};
/// Invalid or unsupported intra-picture data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntraPictureError;
impl fmt::Display for IntraPictureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid or unsupported TVC intra picture.")
    }
}
impl error::Error for IntraPictureError {}
/// A decoded, display-cropped planar 8-bit 4:2:0 picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame420 {
    /// Display width.
    pub width: u16,
    /// Display height.
    pub height: u16,
    /// Tightly packed luma plane.
    pub y: Vec<u8>,
    /// Tightly packed Cb plane (rounded up for odd dimensions).
    pub cb: Vec<u8>,
    /// Tightly packed Cr plane (rounded up for odd dimensions).
    pub cr: Vec<u8>,
}
fn block(
    plane: &mut [u8],
    stride: usize,
    origin: (usize, usize),
    top: usize,
    mode: u8,
    mb: &IntraMacroblock,
    b: usize,
) -> Result<(), IntraPictureError> {
    let (bx, by) = origin;
    let t = if by > top {
        let mut a = [0; 8];
        a.copy_from_slice(&plane[(by - 1) * stride + bx..(by - 1) * stride + bx + 8]);
        Some(a)
    } else {
        None
    };
    let l = if bx > 0 {
        let mut a = [0; 8];
        for i in 0..8 {
            a[i] = plane[(by + i) * stride + bx - 1];
        }
        Some(a)
    } else {
        None
    };
    let mode = match mode {
        0 => IntraMode::Dc,
        1 => IntraMode::Vertical,
        2 => IntraMode::Horizontal,
        3 => IntraMode::Planar,
        _ => return Err(IntraPictureError),
    };
    let prediction = intra_8x8(mode, t.as_ref(), l.as_ref());
    let mut residual = [[0i32; 8]; 8];
    if mb.coded & (1 << b) != 0 {
        if b < 4 && mb.transform_4x4 & (1 << b) != 0 {
            for k in 0..4 {
                if mb.subblocks_4x4 & (1 << (4 * b + k)) != 0 {
                    let coeff: &[i16; 16] = mb.coefficients[b][16 * k..16 * k + 16]
                        .try_into()
                        .map_err(|_| IntraPictureError)?;
                    let values = inverse_4x4(coeff, mb.qp).map_err(|_| IntraPictureError)?;
                    for yy in 0..4 {
                        for xx in 0..4 {
                            residual[(k / 2) * 4 + yy][(k % 2) * 4 + xx] = values[yy][xx];
                        }
                    }
                }
            }
        } else {
            residual = inverse_8x8(&mb.coefficients[b], mb.qp).map_err(|_| IntraPictureError)?;
        }
    }
    for yy in 0..8 {
        for xx in 0..8 {
            plane[(by + yy) * stride + bx + xx] =
                (i32::from(prediction[yy][xx]) + residual[yy][xx]).clamp(0, 255) as u8;
        }
    }
    Ok(())
}
fn crop(data: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        out.extend_from_slice(&data[y * stride..y * stride + width]);
    }
    out
}
/// Decode one intra picture (including all slices) into cropped YUV 4:2:0.
/// P and B pictures are not supported yet. This does not validate the complete
/// TVC stream or TNV3 container, and assumes the caller has supplied the picture.
pub fn decode_intra_picture(
    data: &[u8],
    width: u16,
    height: u16,
) -> Result<Frame420, IntraPictureError> {
    if width == 0 || height == 0 || width > 800 || height > 720 {
        return Err(IntraPictureError);
    }
    let w = (usize::from(width) + 15) & !15;
    let h = (usize::from(height) + 15) & !15;
    let (mw, mh) = (w / 16, h / 16);
    let header = PictureHeader::parse(data, mh as u8).map_err(|_| IntraPictureError)?;
    if header.kind != PictureType::Intra {
        return Err(IntraPictureError);
    }
    let rows = usize::from(header.rows_per_slice.unwrap_or(mh as u8));
    let mut y = vec![0; w * h];
    let mut cb = vec![0; w * h / 4];
    let mut cr = vec![0; w * h / 4];
    let blank = DeblockMacroblock {
        intra: true,
        uses: 0,
        motion: [[0; 2]; 2],
        coded: 0,
        qp: header.qp,
    };
    let mut mbs = vec![blank; mw * mh];
    for (slice, range) in header.slices.iter().enumerate() {
        let y0 = slice * rows;
        let count = rows.min(mh - y0);
        let mut dec = IntraSliceDecoder::new(
            &data[range.clone()],
            mw,
            count,
            header.qp,
            header.adaptive_quantization,
            header.transform_4x4,
        )
        .map_err(|_| IntraPictureError)?;
        for my in y0..y0 + count {
            for mx in 0..mw {
                let mb = dec
                    .next_macroblock()
                    .map_err(|_| IntraPictureError)?
                    .ok_or(IntraPictureError)?;
                for b in 0..4 {
                    block(
                        &mut y,
                        w,
                        (mx * 16 + (b % 2) * 8, my * 16 + (b / 2) * 8),
                        y0 * 16,
                        mb.luma_modes[b],
                        &mb,
                        b,
                    )?;
                }
                block(
                    &mut cb,
                    w / 2,
                    (mx * 8, my * 8),
                    y0 * 8,
                    mb.chroma_mode,
                    &mb,
                    4,
                )?;
                block(
                    &mut cr,
                    w / 2,
                    (mx * 8, my * 8),
                    y0 * 8,
                    mb.chroma_mode,
                    &mb,
                    5,
                )?;
                mbs[my * mw + mx] = DeblockMacroblock {
                    intra: true,
                    uses: 0,
                    motion: [[0; 2]; 2],
                    coded: mb.coded,
                    qp: mb.qp,
                };
            }
        }
    }
    deblock(&mut y, &mut cb, &mut cr, w, h, &mbs, header.quarter_pixel)
        .map_err(|_| IntraPictureError)?;
    let dw = usize::from(width);
    let dh = usize::from(height);
    let cw = dw.div_ceil(2);
    let ch = dh.div_ceil(2);
    Ok(Frame420 {
        width,
        height,
        y: crop(&y, w, dw, dh),
        cb: crop(&cb, w / 2, cw, ch),
        cr: crop(&cr, w / 2, cw, ch),
    })
}

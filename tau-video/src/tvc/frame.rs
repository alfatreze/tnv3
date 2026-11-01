//! Reconstruction and display ordering of TVC pictures.
use super::{
    deblock::{DeblockMacroblock, deblock},
    inter::{Interpolation, ReferencePlane, average},
    intra::{IntraMacroblock, IntraSliceDecoder, MacroblockKind},
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
        f.write_str("Invalid or unsupported TVC picture.")
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
    apply_block(plane, stride, origin, mb, b, &prediction)
}
fn apply_block(
    plane: &mut [u8],
    stride: usize,
    origin: (usize, usize),
    mb: &IntraMacroblock,
    b: usize,
    prediction: &[[u8; 8]; 8],
) -> Result<(), IntraPictureError> {
    let (bx, by) = origin;
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
/// A coded-size reference picture, preserving the padded edges required by motion prediction.
#[derive(Clone, Debug)]
pub struct CodedPicture {
    width: u16,
    height: u16,
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}
impl CodedPicture {
    pub(crate) fn from_aligned_frame(frame: &Frame420) -> Result<Self, IntraPictureError> {
        let (w, h) = (usize::from(frame.width), usize::from(frame.height));
        if w == 0
            || h == 0
            || w > 800
            || h > 720
            || w % 16 != 0
            || h % 16 != 0
            || frame.y.len() != w * h
            || frame.cb.len() != w * h / 4
            || frame.cr.len() != w * h / 4
        {
            return Err(IntraPictureError);
        }
        Ok(Self {
            width: frame.width,
            height: frame.height,
            y: frame.y.clone(),
            cb: frame.cb.clone(),
            cr: frame.cr.clone(),
        })
    }

    /// Copy the actual reconstructed coded edges for encoder prediction.
    pub(crate) fn padded_frame(&self) -> Frame420 {
        Frame420 {
            width: (self.width + 15) & !15,
            height: (self.height + 15) & !15,
            y: self.y.clone(),
            cb: self.cb.clone(),
            cr: self.cr.clone(),
        }
    }

    /// Return a cropped picture suitable for presentation.
    pub fn display_frame(&self) -> Frame420 {
        let w = (usize::from(self.width) + 15) & !15;
        let dw = usize::from(self.width);
        let dh = usize::from(self.height);
        Frame420 {
            width: self.width,
            height: self.height,
            y: crop(&self.y, w, dw, dh),
            cb: crop(&self.cb, w / 2, dw.div_ceil(2), dh.div_ceil(2)),
            cr: crop(&self.cr, w / 2, dw.div_ceil(2), dh.div_ceil(2)),
        }
    }
}
/// Decode one intra picture into cropped YUV 4:2:0.
pub fn decode_intra_picture(
    data: &[u8],
    width: u16,
    height: u16,
) -> Result<Frame420, IntraPictureError> {
    if data.first() != Some(&0) {
        return Err(IntraPictureError);
    }
    Ok(decode_picture(data, width, height, [None, None])?.display_frame())
}
/// Decode one picture in decoding order. For P pictures, list 0 is the latest
/// anchor; for B pictures, list 0 is the previous anchor and list 1 the latest.
/// This does not validate complete TVC streams or TNV3 containers.
pub fn decode_picture(
    data: &[u8],
    width: u16,
    height: u16,
    refs: [Option<&CodedPicture>; 2],
) -> Result<CodedPicture, IntraPictureError> {
    if width == 0 || height == 0 || width > 800 || height > 720 {
        return Err(IntraPictureError);
    }
    let w = (usize::from(width) + 15) & !15;
    let h = (usize::from(height) + 15) & !15;
    let (mw, mh) = (w / 16, h / 16);
    let header = PictureHeader::parse(data, mh as u8).map_err(|_| IntraPictureError)?;
    let required = match header.kind {
        PictureType::Intra => 0,
        PictureType::Predictive => 1,
        PictureType::Bidirectional => 2,
    };
    for reference in refs.iter().take(required) {
        let r = reference.ok_or(IntraPictureError)?;
        if r.width != width || r.height != height {
            return Err(IntraPictureError);
        }
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
        let mut dec = IntraSliceDecoder::for_picture(
            &data[range.clone()],
            mw,
            count,
            header.qp,
            header.adaptive_quantization,
            header.transform_4x4,
            header.kind,
        )
        .map_err(|_| IntraPictureError)?;
        for my in y0..y0 + count {
            for mx in 0..mw {
                let mb = dec
                    .next_macroblock()
                    .map_err(|_| IntraPictureError)?
                    .ok_or(IntraPictureError)?;
                if mb.kind == MacroblockKind::Intra {
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
                } else {
                    for b in 0..6 {
                        let (stride, plane_w, plane_h, origin) = if b < 4 {
                            (w, w, h, (mx * 16 + (b % 2) * 8, my * 16 + (b / 2) * 8))
                        } else {
                            (w / 2, w / 2, h / 2, (mx * 8, my * 8))
                        };
                        let mut predictions = [[0u8; 256]; 2];
                        let mut count = 0;
                        for (list, reference) in refs.iter().enumerate() {
                            if mb.uses & (1 << list) == 0 {
                                continue;
                            }
                            let r = reference.ok_or(IntraPictureError)?;
                            let source = if b < 4 {
                                &r.y
                            } else if b == 4 {
                                &r.cb
                            } else {
                                &r.cr
                            };
                            let plane = ReferencePlane::new(source, plane_w, plane_h, stride)
                                .map_err(|_| IntraPictureError)?;
                            let mode = if header.quarter_pixel {
                                if b < 4 {
                                    Interpolation::QuarterLuma
                                } else {
                                    Interpolation::EighthChroma
                                }
                            } else {
                                Interpolation::HalfSample
                            };
                            let vector = mb.motion[list];
                            let motion = if !header.quarter_pixel && b >= 4 {
                                (i32::from(vector[0]) / 2, i32::from(vector[1]) / 2)
                            } else {
                                (i32::from(vector[0]), i32::from(vector[1]))
                            };
                            predictions[count] = plane
                                .predict((origin.0 as i32, origin.1 as i32), motion, (8, 8), mode)
                                .map_err(|_| IntraPictureError)?;
                            count += 1;
                        }
                        if count == 0 {
                            return Err(IntraPictureError);
                        }
                        let mut pred = [[0u8; 8]; 8];
                        for (yy, row) in pred.iter_mut().enumerate() {
                            for (xx, value) in row.iter_mut().enumerate() {
                                let i = yy * 16 + xx;
                                *value = if count == 2 {
                                    average(predictions[0][i], predictions[1][i])
                                } else {
                                    predictions[0][i]
                                };
                            }
                        }
                        let plane = if b < 4 {
                            &mut y
                        } else if b == 4 {
                            &mut cb
                        } else {
                            &mut cr
                        };
                        apply_block(plane, stride, origin, &mb, b, &pred)?;
                    }
                }
                mbs[my * mw + mx] = DeblockMacroblock {
                    intra: mb.kind == MacroblockKind::Intra,
                    uses: mb.uses,
                    motion: mb.motion,
                    coded: mb.coded,
                    qp: mb.qp,
                };
            }
        }
    }
    deblock(&mut y, &mut cb, &mut cr, w, h, &mbs, header.quarter_pixel)
        .map_err(|_| IntraPictureError)?;
    Ok(CodedPicture {
        width,
        height,
        y,
        cb,
        cr,
    })
}

/// Stateful TVC picture decoder that returns pictures in display order.
///
/// Supply complete TVC pictures in decoding order. This helper keeps the two
/// padded anchor pictures required for P/B decoding; it does not parse or
/// validate a TVC file or TNV3 container. Call [`Self::finish`] at end of input.
pub struct PictureDecoder {
    width: u16,
    height: u16,
    previous: Option<CodedPicture>,
    latest: Option<CodedPicture>,
}
impl PictureDecoder {
    /// Create a decoder for a fixed display size.
    pub fn new(width: u16, height: u16) -> Result<Self, IntraPictureError> {
        if width == 0 || height == 0 || width > 800 || height > 720 {
            return Err(IntraPictureError);
        }
        Ok(Self {
            width,
            height,
            previous: None,
            latest: None,
        })
    }
    /// Decode one picture; an anchor may be held until the next anchor arrives.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Frame420>, IntraPictureError> {
        let kind = *data.first().ok_or(IntraPictureError)?;
        let references = if kind == 2 {
            [self.previous.as_ref(), self.latest.as_ref()]
        } else {
            [self.latest.as_ref(), None]
        };
        let picture = decode_picture(data, self.width, self.height, references)?;
        if kind == 2 {
            return Ok(Some(picture.display_frame()));
        }
        let old = self.latest.take();
        let output = old.as_ref().map(CodedPicture::display_frame);
        self.previous = old;
        self.latest = Some(picture);
        Ok(output)
    }
    /// Return the last held anchor and reset the decoder's reference state.
    pub fn finish(&mut self) -> Option<Frame420> {
        self.previous = None;
        self.latest.take().map(|p| p.display_frame())
    }
}

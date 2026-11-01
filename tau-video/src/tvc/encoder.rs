//! Deterministic baseline intra-picture TVC encoding.
use super::{
    coeff::{BlockClass, CoefficientContexts, encode_block},
    frame::Frame420,
    predict::{IntraMode, intra_8x8},
    range::{Probability, RangeEncoder},
    transform::{QS, T8, inverse_8x8},
};
use std::{error, fmt};
/// Invalid input or a picture exceeding the current baseline encoder's bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeError;
impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid input or excessive TVC encoded picture size.")
    }
}
impl error::Error for EncodeError {}
fn encode_pixels(
    src: &[u8],
    dims: (usize, usize),
    plane: &mut [u8],
    stride: usize,
    origin: (usize, usize),
    top: usize,
    qp: u8,
) -> ([i16; 64], bool) {
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
    let pred = intra_8x8(IntraMode::Dc, t.as_ref(), l.as_ref());
    let mut input = [[0i64; 8]; 8];
    for y in 0..8 {
        for x in 0..8 {
            let sx = (bx + x).min(dims.0 - 1);
            let sy = (by + y).min(dims.1 - 1);
            input[y][x] = i64::from(src[sy * dims.0 + sx]) - i64::from(pred[y][x]);
        }
    }
    let levels = quantize_8(&input, qp);
    let coded = levels.iter().any(|&x| x != 0);
    let residual = inverse_8x8(&levels, qp).unwrap_or([[0; 8]; 8]);
    for y in 0..8 {
        for x in 0..8 {
            plane[(by + y) * stride + bx + x] =
                (i32::from(pred[y][x]) + residual[y][x]).clamp(0, 255) as u8;
        }
    }
    (levels, coded)
}
fn quantize_8(input: &[[i64; 8]; 8], qp: u8) -> [i16; 64] {
    // A separable integer transform computes the same unrounded sums using
    // two 1-D passes instead of a four-deep direct transform.
    let mut temp = [[0i64; 8]; 8];
    for u in 0..8 {
        for x in 0..8 {
            for y in 0..8 {
                temp[u][x] += T8[u][y] * input[y][x];
            }
        }
    }
    let mut transformed = [[0i64; 8]; 8];
    for u in 0..8 {
        for v in 0..8 {
            for x in 0..8 {
                transformed[u][v] += T8[v][x] * temp[u][x];
            }
        }
    }
    let mut levels = [0i16; 64];
    let mut n = 0;
    let denom = 32768 * QS[usize::from(qp)];
    for sum in 0usize..15 {
        let lo = sum.saturating_sub(7);
        let hi = sum.min(7);
        for off in 0..=hi - lo {
            let row = if sum % 2 == 1 { lo + off } else { hi - off };
            let v = transformed[row][sum - row];
            let q = ((v.abs() * 16 + denom / 2) / denom).min(30000) as i16;
            levels[n] = if v < 0 { -q } else { q };
            n += 1;
        }
    }
    levels
}
/// Encode a single TVC intra picture from tightly packed YUV420 planes.
///
/// This first deterministic implementation uses DC intra prediction, 8×8
/// transforms and one macroblock row per slice. It does not perform motion
/// search, rate control, adaptive quantization, or optimized mode selection.
/// The caller is responsible for TVC level and rate-window validation.
pub fn encode_intra_picture(frame: &Frame420, qp: u8) -> Result<Vec<u8>, EncodeError> {
    let (dw, dh) = (usize::from(frame.width), usize::from(frame.height));
    if dw == 0 || dh == 0 || dw > 800 || dh > 720 || qp > 51 {
        return Err(EncodeError);
    }
    let (cw, ch) = (dw.div_ceil(2), dh.div_ceil(2));
    if frame.y.len() != dw * dh || frame.cb.len() != cw * ch || frame.cr.len() != cw * ch {
        return Err(EncodeError);
    }
    let (w, h) = ((dw + 15) & !15, (dh + 15) & !15);
    let (mw, mh) = (w / 16, h / 16);
    let mut planes = [vec![0u8; w * h], vec![0u8; w * h / 4], vec![0u8; w * h / 4]];
    let mut slices = Vec::with_capacity(mh);
    for my in 0..mh {
        let mut coder = RangeEncoder::new();
        let p = Probability::default();
        let mut mpm = p;
        let mut cmode = p;
        let mut cbp = [[p; 3]; 3];
        let mut coeff = CoefficientContexts::default();
        let mut left_coded = 0u8;
        for mx in 0..mw {
            let mut levels = [[0i16; 64]; 6];
            let mut coded = 0u8;
            for (b, lev) in levels.iter_mut().enumerate() {
                let (source, dims, stride, origin, top, plane) = if b < 4 {
                    (
                        &frame.y[..],
                        (dw, dh),
                        w,
                        (mx * 16 + (b % 2) * 8, my * 16 + (b / 2) * 8),
                        my * 16,
                        0,
                    )
                } else if b == 4 {
                    (&frame.cb[..], (cw, ch), w / 2, (mx * 8, my * 8), my * 8, 1)
                } else {
                    (&frame.cr[..], (cw, ch), w / 2, (mx * 8, my * 8), my * 8, 2)
                };
                let (value, yes) =
                    encode_pixels(source, dims, &mut planes[plane], stride, origin, top, qp);
                *lev = value;
                if yes {
                    coded |= 1 << b;
                }
            }
            for _ in 0..4 {
                coder.bit(&mut mpm, true);
            }
            coder.bit(&mut cmode, false);
            for b in 0..6 {
                let (l, t, class) = if b < 4 {
                    (
                        if b & 1 != 0 {
                            (coded >> (b - 1)) & 1
                        } else {
                            (left_coded >> (b + 1)) & 1
                        },
                        if b & 2 != 0 {
                            (coded >> (b - 2)) & 1
                        } else {
                            0
                        },
                        0,
                    )
                } else {
                    ((left_coded >> b) & 1, 0, b - 3)
                };
                coder.bit(&mut cbp[class][usize::from(l + t)], coded & (1 << b) != 0);
            }
            for (b, lev) in levels.iter().enumerate() {
                if coded & (1 << b) != 0 {
                    encode_block(
                        &mut coder,
                        &mut coeff,
                        if b < 4 {
                            BlockClass::Luma8
                        } else {
                            BlockClass::Chroma8
                        },
                        lev,
                    )
                    .map_err(|_| EncodeError)?;
                }
            }
            left_coded = coded;
        }
        slices.push(coder.finish());
    }
    let mut output = vec![0, qp, if mh > 1 { 8 } else { 0 }];
    if mh > 1 {
        output.push(1);
        for slice in slices.iter().take(mh - 1) {
            let len = u32::try_from(slice.len()).map_err(|_| EncodeError)?;
            output.extend_from_slice(&len.to_le_bytes());
        }
    }
    for slice in slices {
        output.extend_from_slice(&slice);
    }
    if output.len() > 384_000 {
        return Err(EncodeError);
    }
    Ok(output)
}

// A P picture containing only zero-motion skip blocks. Each row is its own
// slice, so its first macroblock has no left or top skip context.
fn encode_skip_picture(width: usize, height: usize, qp: u8) -> Vec<u8> {
    let mw = width.div_ceil(16);
    let mh = height.div_ceil(16);
    let mut slices = Vec::with_capacity(mh);
    for _ in 0..mh {
        let mut r = RangeEncoder::new();
        let mut skip = [Probability::default(); 3];
        for x in 0..mw {
            r.bit(&mut skip[usize::from(x > 0)], true);
        }
        slices.push(r.finish());
    }
    let mut out = vec![1, qp, if mh > 1 { 8 } else { 0 }];
    if mh > 1 {
        out.push(1);
        for slice in slices.iter().take(mh - 1) {
            out.extend_from_slice(&(slice.len() as u32).to_le_bytes());
        }
    }
    for slice in slices {
        out.extend_from_slice(&slice);
    }
    out
}
// Encode a whole-picture integer translation as zero-residual inter blocks.
fn encode_translation_picture(
    width: usize,
    height: usize,
    qp: u8,
    dx: i32,
    dy: i32,
) -> Result<Vec<u8>, EncodeError> {
    let mw = width.div_ceil(16);
    let mh = height.div_ceil(16);
    let mut slices = Vec::with_capacity(mh);
    for _ in 0..mh {
        let mut r = RangeEncoder::new();
        let p = Probability::default();
        let mut skip = p;
        let mut intra = p;
        let mut zero = [[p; 3]; 2];
        let mut mag = [[p; 6]; 2];
        let mut cbp = [[p; 3]; 3];
        let mut prev = [0i32; 2];
        for x in 0..mw {
            r.bit(&mut skip, false);
            r.bit(&mut intra, false);
            for k in 0..2 {
                let diff = if x == 0 { [dx * 2, dy * 2][k] } else { 0 };
                let pd = prev[k].unsigned_abs();
                let class = if pd < 2 {
                    0
                } else if pd < 16 {
                    1
                } else {
                    2
                };
                super::signed::encode_signed(&mut r, &mut zero[k][class], &mut mag[k], diff)
                    .map_err(|_| EncodeError)?;
                prev[k] = diff;
            }
            for b in 0..6 {
                let class = if b < 4 { 0 } else { b - 3 };
                r.bit(&mut cbp[class][0], false);
            }
        }
        slices.push(r.finish());
    }
    let mut out = vec![1, qp, if mh > 1 { 8 } else { 0 }];
    if mh > 1 {
        out.push(1);
        for slice in slices.iter().take(mh - 1) {
            out.extend_from_slice(&(slice.len() as u32).to_le_bytes());
        }
    }
    for slice in slices {
        out.extend_from_slice(&slice);
    }
    Ok(out)
}
fn shifted_plane(src: &[u8], width: usize, height: usize, dx: i32, dy: i32) -> Vec<u8> {
    let mut out = vec![0; src.len()];
    for y in 0..height {
        for x in 0..width {
            let sx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
            let sy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
            out[y * width + x] = src[sy * width + sx];
        }
    }
    out
}
fn shift_reference(reference: &Frame420, dx: i32, dy: i32) -> Frame420 {
    let (w, h) = (usize::from(reference.width), usize::from(reference.height));
    let (cw, ch) = (w / 2, h / 2);
    Frame420 {
        width: reference.width,
        height: reference.height,
        y: shifted_plane(&reference.y, w, h, dx, dy),
        cb: shifted_plane(&reference.cb, cw, ch, dx / 2, dy / 2),
        cr: shifted_plane(&reference.cr, cw, ch, dx / 2, dy / 2),
    }
}
// Search integer translations on a sparse luma grid, then verify the entire
// YUV frame against the same error bound used for zero-motion prediction.
fn find_translation(current: &Frame420, reference: &Frame420) -> Option<(i32, i32, Frame420)> {
    let (w, h) = (usize::from(current.width), usize::from(current.height));
    if w % 16 != 0 || h % 16 != 0 {
        return None;
    }
    let mut best = (u64::MAX, 0, 0);
    for dy in (-8i32..=8).step_by(2) {
        for dx in (-8i32..=8).step_by(2) {
            if dx == 0 && dy == 0 {
                continue;
            }
            let mut score = 0u64;
            for y in (0..h).step_by(8) {
                for x in (0..w).step_by(8) {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let d = i32::from(current.y[y * w + x]) - i32::from(reference.y[sy * w + sx]);
                    score += (d * d) as u64;
                }
            }
            if score < best.0 {
                best = (score, dx, dy);
            }
        }
    }
    // A coarse rejection avoids building a full-frame candidate for clearly
    // unrelated pictures. This may miss a motion opportunity, but cannot
    // bypass the full-frame quality check below.
    let samples = w.div_ceil(8) * h.div_ceil(8);
    if best.0 > samples as u64 * 400 {
        return None;
    }
    let shifted = shift_reference(reference, best.1, best.2);
    Some((best.1, best.2, shifted))
}
// Encode a P picture with a whole-frame integer vector and 8x8 residuals.
// Search 16 luma samples per macroblock over a small even-pixel window.
fn block_motion(frame: &Frame420, reference: &Frame420, mx: usize, my: usize) -> (i32, i32) {
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    let mut best = (u64::MAX, 0, 0);
    for dy in (-4i32..=4).step_by(2) {
        for dx in (-4i32..=4).step_by(2) {
            let mut score = 0u64;
            for y in (0..16).step_by(4) {
                for x in (0..16).step_by(4) {
                    let px = mx * 16 + x;
                    let py = my * 16 + y;
                    let sx = (px as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (py as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let d = i32::from(frame.y[py * w + px]) - i32::from(reference.y[sy * w + sx]);
                    score += (d * d) as u64;
                }
            }
            if score < best.0 {
                best = (score, dx, dy);
            }
        }
    }
    (best.1, best.2)
}
// Conservative precheck: test up to eight spatially distributed macroblocks.
fn block_motion_worthwhile(frame: &Frame420, reference: &Frame420) -> bool {
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    let (mw, mh) = (w / 16, h / 16);
    let total = mw * mh;
    let n = total.min(8);
    let (mut zero, mut best) = (0u64, 0u64);
    for i in 0..n {
        let index = i * total / n;
        let (mx, my) = (index % mw, index / mw);
        let (dx, dy) = block_motion(frame, reference, mx, my);
        for y in (0..16).step_by(4) {
            for x in (0..16).step_by(4) {
                let px = mx * 16 + x;
                let py = my * 16 + y;
                let value = i32::from(frame.y[py * w + px]);
                let dz = value - i32::from(reference.y[py * w + px]);
                let sx = (px as i32 + dx).clamp(0, w as i32 - 1) as usize;
                let sy = (py as i32 + dy).clamp(0, h as i32 - 1) as usize;
                let dm = value - i32::from(reference.y[sy * w + sx]);
                zero += (dz * dz) as u64;
                best += (dm * dm) as u64;
            }
        }
    }
    best * 100 < zero * 85
}
fn encode_residual_picture(
    frame: &Frame420,
    reference: &Frame420,
    qp: u8,
    motion: (i32, i32),
    per_block: bool,
    bidirectional: bool,
) -> Result<Vec<u8>, EncodeError> {
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    if w % 16 != 0
        || h % 16 != 0
        || reference.width != frame.width
        || reference.height != frame.height
    {
        return Err(EncodeError);
    }
    let (mw, mh) = (w / 16, h / 16);
    let mut slices = Vec::with_capacity(mh);
    for my in 0..mh {
        let mut coder = RangeEncoder::new();
        let p = Probability::default();
        let mut skip = p;
        let mut intra = p;
        let mut bdir = p;
        let mut zero = [[p; 3]; 2];
        let mut magnitude = [[p; 6]; 2];
        let mut previous_diff = [0i32; 2];
        let mut previous_motion = (0i32, 0i32);
        let mut cbp = [[p; 3]; 3];
        let mut coeff = CoefficientContexts::default();
        let mut left_coded = 0u8;
        for mx in 0..mw {
            let current_motion = if per_block {
                block_motion(frame, reference, mx, my)
            } else {
                motion
            };
            let mut levels = [[0i16; 64]; 6];
            let mut coded = 0u8;
            for (b, block) in levels.iter_mut().enumerate() {
                let (src, pred, stride, origin) = if b < 4 {
                    (
                        &frame.y,
                        &reference.y,
                        w,
                        (mx * 16 + (b % 2) * 8, my * 16 + (b / 2) * 8),
                    )
                } else if b == 4 {
                    (&frame.cb, &reference.cb, w / 2, (mx * 8, my * 8))
                } else {
                    (&frame.cr, &reference.cr, w / 2, (mx * 8, my * 8))
                };
                let mut residual = [[0i64; 8]; 8];
                for (y, row) in residual.iter_mut().enumerate() {
                    for (x, value) in row.iter_mut().enumerate() {
                        let i = (origin.1 + y) * stride + origin.0 + x;
                        let (dx, dy) = if per_block {
                            if b < 4 {
                                current_motion
                            } else {
                                (current_motion.0 / 2, current_motion.1 / 2)
                            }
                        } else {
                            (0, 0)
                        };
                        let plane_h = if b < 4 { h } else { h / 2 };
                        let px = ((origin.0 + x) as i32 + dx).clamp(0, stride as i32 - 1) as usize;
                        let py = ((origin.1 + y) as i32 + dy).clamp(0, plane_h as i32 - 1) as usize;
                        *value = i64::from(src[i]) - i64::from(pred[py * stride + px]);
                    }
                }
                *block = quantize_8(&residual, qp);
                if block.iter().any(|&v| v != 0) {
                    coded |= 1 << b;
                }
            }
            coder.bit(&mut skip, false);
            coder.bit(&mut intra, false);
            if bidirectional {
                coder.bit(&mut bdir, false);
            }
            for component in 0..2 {
                let current = if component == 0 {
                    current_motion.0
                } else {
                    current_motion.1
                };
                let previous = if component == 0 {
                    previous_motion.0
                } else {
                    previous_motion.1
                };
                let diff = (current - previous) * 2;
                let magnitude_before = previous_diff[component].unsigned_abs();
                let class = if magnitude_before < 2 {
                    0
                } else if magnitude_before < 16 {
                    1
                } else {
                    2
                };
                super::signed::encode_signed(
                    &mut coder,
                    &mut zero[component][class],
                    &mut magnitude[component],
                    diff,
                )
                .map_err(|_| EncodeError)?;
                previous_diff[component] = diff;
            }
            if bidirectional {
                for context in &mut zero {
                    coder.bit(&mut context[0], false);
                }
            }
            previous_motion = current_motion;
            for b in 0..6 {
                let (l, t, class) = if b < 4 {
                    (
                        if b & 1 != 0 {
                            (coded >> (b - 1)) & 1
                        } else {
                            (left_coded >> (b + 1)) & 1
                        },
                        if b & 2 != 0 {
                            (coded >> (b - 2)) & 1
                        } else {
                            0
                        },
                        0,
                    )
                } else {
                    ((left_coded >> b) & 1, 0, b - 3)
                };
                coder.bit(&mut cbp[class][usize::from(l + t)], coded & (1 << b) != 0);
            }
            for (b, block) in levels.iter().enumerate() {
                if coded & (1 << b) != 0 {
                    encode_block(
                        &mut coder,
                        &mut coeff,
                        if b < 4 {
                            BlockClass::Luma8
                        } else {
                            BlockClass::Chroma8
                        },
                        block,
                    )
                    .map_err(|_| EncodeError)?;
                }
            }
            left_coded = coded;
        }
        slices.push(coder.finish());
    }
    let mut out = vec![
        if bidirectional { 2 } else { 1 },
        qp,
        if mh > 1 { 8 } else { 0 },
    ];
    if mh > 1 {
        out.push(1);
        for slice in slices.iter().take(mh - 1) {
            out.extend_from_slice(&(slice.len() as u32).to_le_bytes());
        }
    }
    for slice in slices {
        out.extend_from_slice(&slice);
    }
    if out.len() > 384_000 {
        return Err(EncodeError);
    }
    Ok(out)
}
fn squared_error(a: &Frame420, b: &Frame420) -> u64 {
    [(&a.y, &b.y), (&a.cb, &b.cb), (&a.cr, &b.cr)]
        .into_iter()
        .flat_map(|(a, b)| a.iter().zip(b.iter()))
        .map(|(&a, &b)| {
            let d = i32::from(a) - i32::from(b);
            (d * d) as u64
        })
        .sum()
}
// Require a low mean squared error over all planes, bounded in integer math.
fn close_to_reference(current: &Frame420, reference: &Frame420) -> bool {
    let mut sum = 0u64;
    let mut count = 0u64;
    let budget = ((current.y.len() + current.cb.len() + current.cr.len()) as u64) * 4;
    for (a, b) in [
        (&current.y, &reference.y),
        (&current.cb, &reference.cb),
        (&current.cr, &reference.cr),
    ] {
        for (&x, &y) in a.iter().zip(b.iter()) {
            let d = i32::from(x) - i32::from(y);
            sum += (d * d) as u64;
            count += 1;
            if sum > budget {
                return false;
            }
        }
    }
    sum <= count * 4
}
/// Three pictures in TVC decoding order: first anchor, future anchor, B picture.
#[cfg(feature = "b-pictures")]
#[derive(Clone, Debug)]
pub struct BPictureGroup {
    /// First input picture, encoded as an intra anchor.
    pub first_anchor: Vec<u8>,
    /// Third input picture, encoded as the future intra anchor.
    pub future_anchor: Vec<u8>,
    /// Middle input picture, encoded as a bidirectional skip picture.
    pub middle_picture: Vec<u8>,
}
/// Try encoding three display-order frames using a bidirectional skip picture.
///
/// Uses a skip picture when the middle picture is close to the reconstructed
/// anchors' average; otherwise tries an 8×8 residual with size/error checks.
/// Returns `Ok(None)` when neither candidate passes. The
/// caller must enforce stream level/rate limits and write the returned pictures
/// in decoding order. This experimental group is not used by `encode_y4m`.
#[cfg(feature = "b-pictures")]
pub fn try_encode_b_group(
    first: &Frame420,
    middle: &Frame420,
    last: &Frame420,
    qp: u8,
) -> Result<Option<BPictureGroup>, EncodeError> {
    if first.width != middle.width
        || first.height != middle.height
        || first.width != last.width
        || first.height != last.height
        || middle.y.len() != first.y.len()
        || middle.cb.len() != first.cb.len()
        || middle.cr.len() != first.cr.len()
    {
        return Err(EncodeError);
    }
    let a = encode_intra_picture(first, qp)?;
    let b = encode_intra_picture(last, qp)?;
    let ar = super::frame::decode_intra_picture(&a, first.width, first.height)
        .map_err(|_| EncodeError)?;
    let br =
        super::frame::decode_intra_picture(&b, last.width, last.height).map_err(|_| EncodeError)?;
    let Some(picture) = try_b_between(&ar, middle, &br, qp)? else {
        return Ok(None);
    };
    Ok(Some(BPictureGroup {
        first_anchor: a,
        future_anchor: b,
        middle_picture: picture,
    }))
}
#[cfg(feature = "b-pictures")]
fn try_b_between(
    ar: &Frame420,
    middle: &Frame420,
    br: &Frame420,
    qp: u8,
) -> Result<Option<Vec<u8>>, EncodeError> {
    fn average(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter()
            .zip(b)
            .map(|(&x, &y)| ((u16::from(x) + u16::from(y) + 1) >> 1) as u8)
            .collect()
    }
    let predicted = Frame420 {
        width: ar.width,
        height: ar.height,
        y: average(&ar.y, &br.y),
        cb: average(&ar.cb, &br.cb),
        cr: average(&ar.cr, &br.cr),
    };
    let picture = if close_to_reference(middle, &predicted) {
        let mut picture = encode_skip_picture(usize::from(ar.width), usize::from(ar.height), qp);
        picture[0] = 2;
        picture
    } else {
        if usize::from(ar.width) % 16 != 0 || usize::from(ar.height) % 16 != 0 {
            return Ok(None);
        }
        let candidate = encode_residual_picture(middle, &predicted, qp, (0, 0), false, true)?;
        let intra = encode_intra_picture(middle, qp)?;
        if candidate.len() >= intra.len() {
            return Ok(None);
        }
        let left = super::frame::CodedPicture::from_aligned_frame(ar).map_err(|_| EncodeError)?;
        let right = super::frame::CodedPicture::from_aligned_frame(br).map_err(|_| EncodeError)?;
        let recon = super::frame::decode_picture(
            &candidate,
            ar.width,
            ar.height,
            [Some(&left), Some(&right)],
        )
        .map_err(|_| EncodeError)?
        .display_frame();
        let intra_recon = super::frame::decode_intra_picture(&intra, ar.width, ar.height)
            .map_err(|_| EncodeError)?;
        let count = (middle.y.len() + middle.cb.len() + middle.cr.len()) as u64;
        if squared_error(middle, &recon) > squared_error(middle, &intra_recon) + count {
            return Ok(None);
        }
        candidate
    };
    Ok(Some(picture))
}

/// Encoded TVC stream statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeSummary {
    /// Number of frames written.
    pub frames: u32,
    /// Selected TVC level (1 through 3).
    pub level: u8,
    /// Number of frames retried at a higher quantizer for level limits.
    pub reencoded: u32,
    /// Pictures encoded using zero-motion prediction.
    pub predicted: u32,
}
/// Stream encoding failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum StreamEncodeError {
    /// Input or output failure.
    Io(std::io::Error),
    /// Invalid, unsupported, or excessive input.
    Invalid(&'static str),
}
impl fmt::Display for StreamEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Invalid(m) => f.write_str(m),
        }
    }
}
impl error::Error for StreamEncodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<std::io::Error> for StreamEncodeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
fn line<R: std::io::BufRead>(input: &mut R) -> Result<Option<Vec<u8>>, StreamEncodeError> {
    let mut result = Vec::new();
    loop {
        let mut b = [0u8; 1];
        match input.read(&mut b)? {
            0 if result.is_empty() => return Ok(None),
            0 => return Err(StreamEncodeError::Invalid("Truncated Y4M line.")),
            _ => {
                result.push(b[0]);
                if result.len() > 4096 {
                    return Err(StreamEncodeError::Invalid("Y4M line exceeds limit."));
                }
                if b[0] == b'\n' {
                    return Ok(Some(result));
                }
            }
        }
    }
}
/// Encode a YUV4MPEG2 8-bit 4:2:0 stream into a TVC1 elementary stream.
///
/// The baseline uses intra pictures and limited zero-motion P pictures, and holds
/// one input frame and one encoded picture
/// at a time. Output must be seekable so its frame count can be filled in. On
/// error the caller must discard the partially written output. The encoder
/// selects the smallest fitting TVC level and retries with a higher QP if the
/// picture or rolling video-byte limit would be exceeded.
pub fn encode_y4m<R: std::io::BufRead, W: std::io::Write + std::io::Seek>(
    input: R,
    output: W,
    qp: u8,
) -> Result<EncodeSummary, StreamEncodeError> {
    encode_y4m_with_progress(input, output, qp, |_| {})
}

/// Encode a Y4M stream and call `progress` after each successfully written picture.
///
/// The callback receives the cumulative frame count. It runs synchronously on the
/// encoding thread and must return promptly. The output is identical to `encode_y4m`.
pub fn encode_y4m_with_progress<R, W, F>(
    mut input: R,
    mut output: W,
    qp: u8,
    mut progress: F,
) -> Result<EncodeSummary, StreamEncodeError>
where
    R: std::io::BufRead,
    W: std::io::Write + std::io::Seek,
    F: FnMut(u32),
{
    use std::io::SeekFrom;
    if qp > 51 {
        return Err(StreamEncodeError::Invalid("Invalid QP."));
    }
    let header = line(&mut input)?.ok_or(StreamEncodeError::Invalid("Missing Y4M header."))?;
    let text = std::str::from_utf8(&header)
        .map_err(|_| StreamEncodeError::Invalid("Invalid Y4M header."))?;
    let mut fields = text.split_ascii_whitespace();
    if fields.next() != Some("YUV4MPEG2") {
        return Err(StreamEncodeError::Invalid("Invalid Y4M magic."));
    }
    let (mut width, mut height, mut num, mut den) = (None, None, None, None);
    for field in fields {
        if let Some(v) = field.strip_prefix('W') {
            width = v.parse::<u16>().ok();
        } else if let Some(v) = field.strip_prefix('H') {
            height = v.parse::<u16>().ok();
        } else if let Some(v) = field.strip_prefix('F') {
            if let Some((a, b)) = v.split_once(':') {
                num = a.parse::<u16>().ok();
                den = b.parse::<u16>().ok();
            }
        } else if let Some(v) = field.strip_prefix('C') {
            if !matches!(v, "420" | "420jpeg" | "420mpeg2" | "420paldv") {
                return Err(StreamEncodeError::Invalid(
                    "Only 8-bit YUV420 input is supported.",
                ));
            }
        } else if let Some(v) = field.strip_prefix('I') {
            if v != "p" && v != "?" {
                return Err(StreamEncodeError::Invalid(
                    "Interlaced input is unsupported.",
                ));
            }
        }
    }
    let (w, h, n, d) = (
        width.ok_or(StreamEncodeError::Invalid("Missing width."))?,
        height.ok_or(StreamEncodeError::Invalid("Missing height."))?,
        num.ok_or(StreamEncodeError::Invalid("Missing frame rate."))?,
        den.ok_or(StreamEncodeError::Invalid("Missing frame rate."))?,
    );
    if w == 0
        || h == 0
        || w > 800
        || h > 720
        || n == 0
        || d == 0
        || u32::from(n) > 30 * u32::from(d)
    {
        return Err(StreamEncodeError::Invalid(
            "Unsupported dimensions or frame rate.",
        ));
    }
    let (cw, ch) = ((u32::from(w) + 15) & !15, (u32::from(h) + 15) & !15);
    let mb = (cw / 16) * (ch / 16);
    let levels = [
        (400, 240, 375, 11250, 96000usize, 200000usize),
        (800, 464, 1450, 43500, 256000, 400000),
        (800, 720, 2250, 67500, 384000, 600000),
    ];
    let (index, limits) = levels
        .iter()
        .enumerate()
        .find(|(_, l)| {
            cw <= l.0
                && ch <= l.1
                && mb <= l.2
                && u64::from(mb) * u64::from(n) <= u64::from(l.3 as u32) * u64::from(d)
        })
        .ok_or(StreamEncodeError::Invalid("No TVC level fits the input."))?;
    let level = (index + 1) as u8;
    let start = output.stream_position()?;
    output.write_all(b"TVC1")?;
    output.write_all(&w.to_le_bytes())?;
    output.write_all(&h.to_le_bytes())?;
    output.write_all(&n.to_le_bytes())?;
    output.write_all(&d.to_le_bytes())?;
    output.write_all(&0u32.to_le_bytes())?;
    output.write_all(&[level, 0, 0, 0])?;
    let (wz, hz) = (usize::from(w), usize::from(h));
    let (chroma_w, chroma_h) = (wz.div_ceil(2), hz.div_ceil(2));
    let mut frame = Frame420 {
        width: w,
        height: h,
        y: vec![0; wz * hz],
        cb: vec![0; chroma_w * chroma_h],
        cr: vec![0; chroma_w * chroma_h],
    };
    let window = usize::from(n).div_ceil(usize::from(d));
    let picture_budget = if cfg!(feature = "safe-rate") {
        limits.4.min(limits.5 / window)
    } else {
        limits.4
    };
    let mut lengths = std::collections::VecDeque::new();
    let mut sum = 0usize;
    let (mut frames, mut reencoded, mut predicted) = (0u32, 0u32, 0u32);
    let mut reference: Option<Frame420> = None;
    while let Some(tag) = line(&mut input)? {
        if tag != b"FRAME\n" && !tag.starts_with(b"FRAME ") {
            return Err(StreamEncodeError::Invalid("Invalid Y4M frame marker."));
        }
        if frames >= crate::MAX_FRAMES {
            return Err(StreamEncodeError::Invalid("Too many frames."));
        }
        input.read_exact(&mut frame.y)?;
        input.read_exact(&mut frame.cb)?;
        input.read_exact(&mut frame.cr)?;
        if lengths.len() == window {
            sum -= lengths
                .pop_front()
                .ok_or(StreamEncodeError::Invalid("Invalid rate window."))?;
        }
        let still = frames % 96 != 0
            && reference
                .as_ref()
                .is_some_and(|r| close_to_reference(&frame, r));
        let translation = if frames % 96 != 0 && !still {
            reference.as_ref().and_then(|r| find_translation(&frame, r))
        } else {
            None
        };
        let prediction = if still {
            Some((0, 0, None))
        } else {
            translation
                .as_ref()
                .filter(|(_, _, p)| close_to_reference(&frame, p))
                .map(|(x, y, p)| (*x, *y, Some(p.clone())))
        };
        let mut q = qp;
        let picture = if let Some((dx, dy, shifted)) = prediction {
            let p = if dx == 0 && dy == 0 {
                encode_skip_picture(wz, hz, qp)
            } else {
                encode_translation_picture(wz, hz, qp, dx, dy)
                    .map_err(|_| StreamEncodeError::Invalid("Could not encode motion picture."))?
            };
            if p.len() > picture_budget || sum + p.len() > limits.5 {
                return Err(StreamEncodeError::Invalid(
                    "Predictive picture exceeds TVC level limits.",
                ));
            }
            if let Some(updated) = shifted {
                reference = Some(updated);
            }
            predicted += 1;
            p
        } else {
            let p = loop {
                match encode_intra_picture(&frame, q) {
                    Ok(p) if p.len() <= picture_budget && sum + p.len() <= limits.5 => break p,
                    _ if q < 51 => q += 1,
                    _ => {
                        return Err(StreamEncodeError::Invalid(
                            "Picture cannot fit TVC level limits.",
                        ));
                    }
                }
            };
            let intra_recon = super::frame::decode_intra_picture(&p, w, h).map_err(|_| {
                StreamEncodeError::Invalid("Encoded picture could not be reconstructed.")
            })?;
            let mut chosen = p;
            let mut chosen_recon = None;
            if frames % 96 != 0 && wz % 16 == 0 && hz % 16 == 0 {
                if let Some(previous) = reference.as_ref() {
                    let anchor = super::frame::CodedPicture::from_aligned_frame(previous)
                        .map_err(|_| StreamEncodeError::Invalid("Invalid prediction reference."))?;
                    let mut candidates = vec![(previous, (0, 0), false)];
                    if let Some((dx, dy, shifted)) = translation.as_ref() {
                        candidates.push((shifted, (*dx, *dy), false));
                    }
                    if cfg!(feature = "block-motion") && block_motion_worthwhile(&frame, previous) {
                        candidates.push((previous, (0, 0), true));
                    }
                    for (predictor, motion, per_block) in candidates {
                        if let Ok(candidate) =
                            encode_residual_picture(&frame, predictor, q, motion, per_block, false)
                        {
                            if candidate.len() >= chosen.len()
                                || candidate.len() > picture_budget
                                || sum + candidate.len() > limits.5
                            {
                                continue;
                            }
                            let recon = super::frame::decode_picture(
                                &candidate,
                                w,
                                h,
                                [Some(&anchor), None],
                            )
                            .map_err(|_| {
                                StreamEncodeError::Invalid(
                                    "Residual picture could not be reconstructed.",
                                )
                            })?
                            .display_frame();
                            let count = (frame.y.len() + frame.cb.len() + frame.cr.len()) as u64;
                            if squared_error(&frame, &recon)
                                <= squared_error(&frame, &intra_recon) + count
                            {
                                chosen_recon = Some(recon);
                                chosen = candidate;
                            }
                        }
                    }
                }
            }
            if let Some(recon) = chosen_recon {
                reference = Some(recon);
                predicted += 1;
            } else {
                reference = Some(intra_recon);
            }
            chosen
        };
        if q != qp {
            reencoded += 1;
        }
        let len = picture.len();
        output.write_all(&(len as u32).to_le_bytes())?;
        output.write_all(&picture)?;
        lengths.push_back(len);
        sum += len;
        frames += 1;
        progress(frames);
    }
    if frames == 0 {
        return Err(StreamEncodeError::Invalid("Y4M stream has no frames."));
    }
    let end = output.stream_position()?;
    output.seek(SeekFrom::Start(start + 12))?;
    output.write_all(&frames.to_le_bytes())?;
    output.seek(SeekFrom::Start(end))?;
    Ok(EncodeSummary {
        frames,
        level,
        reencoded,
        predicted,
    })
}

/// Statistics from experimental three-frame B-picture encoding.
#[cfg(feature = "b-pictures")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BStreamSummary {
    /// Frames encoded.
    pub frames: u32,
    /// Chosen TVC level.
    pub level: u8,
    /// B pictures written.
    pub b_pictures: u32,
    /// Intra pictures retried at a higher quantizer.
    pub reencoded: u32,
}
#[cfg(feature = "b-pictures")]
#[derive(Clone)]
struct BRateWindow {
    window: usize,
    sizes: std::collections::VecDeque<usize>,
    sum: usize,
    max_picture: usize,
    max_rate: usize,
}
#[cfg(feature = "b-pictures")]
impl BRateWindow {
    fn accepts(&self, pictures: &[&[u8]]) -> bool {
        let mut sizes = self.sizes.clone();
        let mut sum = self.sum;
        for p in pictures {
            if p.len() > self.max_picture {
                return false;
            }
            if sizes.len() == self.window {
                if let Some(v) = sizes.pop_front() {
                    sum -= v;
                }
            }
            sum += p.len();
            if sum > self.max_rate {
                return false;
            }
            sizes.push_back(p.len());
        }
        true
    }
    fn write<W: std::io::Write>(&mut self, out: &mut W, p: &[u8]) -> Result<(), StreamEncodeError> {
        if !self.accepts(&[p]) {
            return Err(StreamEncodeError::Invalid(
                "Picture exceeds TVC rate limits.",
            ));
        }
        if self.sizes.len() == self.window {
            self.sum -= self
                .sizes
                .pop_front()
                .ok_or(StreamEncodeError::Invalid("Invalid rate window."))?;
        }
        self.sum += p.len();
        self.sizes.push_back(p.len());
        out.write_all(&(p.len() as u32).to_le_bytes())?;
        out.write_all(p)?;
        Ok(())
    }
}
#[cfg(feature = "b-pictures")]
fn read_b_frame<R: std::io::BufRead>(
    input: &mut R,
    w: u16,
    h: u16,
) -> Result<Option<Frame420>, StreamEncodeError> {
    let Some(tag) = line(input)? else {
        return Ok(None);
    };
    if tag != b"FRAME\n" && !tag.starts_with(b"FRAME ") {
        return Err(StreamEncodeError::Invalid("Invalid Y4M frame marker."));
    }
    let (wz, hz) = (usize::from(w), usize::from(h));
    let (cw, ch) = (wz.div_ceil(2), hz.div_ceil(2));
    let mut f = Frame420 {
        width: w,
        height: h,
        y: vec![0; wz * hz],
        cb: vec![0; cw * ch],
        cr: vec![0; cw * ch],
    };
    input.read_exact(&mut f.y)?;
    input.read_exact(&mut f.cb)?;
    input.read_exact(&mut f.cr)?;
    Ok(Some(f))
}
#[cfg(feature = "b-pictures")]
#[derive(Clone)]
struct BAnchor {
    picture: Vec<u8>,
    reconstructed: Frame420,
    retried: bool,
}
#[cfg(feature = "b-pictures")]
fn choose_b_anchor(
    frame: &Frame420,
    previous: Option<&Frame420>,
    qp: u8,
    window: &BRateWindow,
    force_intra: bool,
) -> Result<BAnchor, StreamEncodeError> {
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    let reference = if force_intra { None } else { previous };
    if let Some(old) = reference {
        if close_to_reference(frame, old) {
            let p = encode_skip_picture(w, h, qp);
            if window.accepts(&[&p]) {
                return Ok(BAnchor {
                    picture: p,
                    reconstructed: old.clone(),
                    retried: false,
                });
            }
        }
        if let Some((dx, dy, shifted)) = find_translation(frame, old) {
            if close_to_reference(frame, &shifted) {
                let p = encode_translation_picture(w, h, qp, dx, dy)
                    .map_err(|_| StreamEncodeError::Invalid("Could not encode motion anchor."))?;
                if window.accepts(&[&p]) {
                    return Ok(BAnchor {
                        picture: p,
                        reconstructed: shifted,
                        retried: false,
                    });
                }
            }
        }
    }
    let mut q = qp;
    let mut chosen = loop {
        match encode_intra_picture(frame, q) {
            Ok(p) if window.accepts(&[&p]) => break p,
            _ if q < 51 => q += 1,
            _ => {
                return Err(StreamEncodeError::Invalid(
                    "Picture cannot fit TVC rate limits.",
                ));
            }
        }
    };
    let intra = super::frame::decode_intra_picture(&chosen, frame.width, frame.height)
        .map_err(|_| StreamEncodeError::Invalid("Intra anchor could not be decoded."))?;
    let mut best = None;
    if w % 16 == 0 && h % 16 == 0 {
        if let Some(old) = reference {
            let anchor = super::frame::CodedPicture::from_aligned_frame(old)
                .map_err(|_| StreamEncodeError::Invalid("Invalid prediction reference."))?;
            let translation = find_translation(frame, old);
            let mut candidates = vec![(old, (0, 0), false)];
            if let Some((dx, dy, shifted)) = translation.as_ref() {
                candidates.push((shifted, (*dx, *dy), false));
            }
            if cfg!(feature = "block-motion") && block_motion_worthwhile(frame, old) {
                candidates.push((old, (0, 0), true));
            }
            let count = (frame.y.len() + frame.cb.len() + frame.cr.len()) as u64;
            let limit = squared_error(frame, &intra) + count;
            for (predictor, motion, per_block) in candidates {
                if let Ok(candidate) =
                    encode_residual_picture(frame, predictor, q, motion, per_block, false)
                {
                    if candidate.len() >= chosen.len() || !window.accepts(&[&candidate]) {
                        continue;
                    }
                    let decoded = super::frame::decode_picture(
                        &candidate,
                        frame.width,
                        frame.height,
                        [Some(&anchor), None],
                    )
                    .map_err(|_| StreamEncodeError::Invalid("P anchor could not be decoded."))?
                    .display_frame();
                    if squared_error(frame, &decoded) <= limit {
                        chosen = candidate;
                        best = Some(decoded);
                    }
                }
            }
        }
    }
    Ok(BAnchor {
        picture: chosen,
        reconstructed: best.unwrap_or(intra),
        retried: q != qp,
    })
}
/// Encode Y4M as an experimental TVC1 stream with bounded three-frame groups.
///
/// The previous anchor is reused with a future I or P anchor; a zero-motion
/// B picture is used only if it meets the quality bound, is smaller than its
/// intra alternative at the same QP, and fits the picture/rate windows. Other
/// pictures fall back to I/P anchor encoding with QP retries. Uses bounded buffers.
/// Output is incremental; discard it on error. This is separate from the normal
/// P-picture encoder and is not production qualified.
#[cfg(feature = "b-pictures")]
pub fn encode_y4m_b<R: std::io::BufRead, W: std::io::Write + std::io::Seek>(
    mut input: R,
    mut output: W,
    qp: u8,
) -> Result<BStreamSummary, StreamEncodeError> {
    use std::io::SeekFrom;
    if qp > 51 {
        return Err(StreamEncodeError::Invalid("Invalid QP."));
    }
    let header = line(&mut input)?.ok_or(StreamEncodeError::Invalid("Missing Y4M header."))?;
    let text = std::str::from_utf8(&header)
        .map_err(|_| StreamEncodeError::Invalid("Invalid Y4M header."))?;
    let mut fields = text.split_ascii_whitespace();
    if fields.next() != Some("YUV4MPEG2") {
        return Err(StreamEncodeError::Invalid("Invalid Y4M magic."));
    }
    let (mut width, mut height, mut num, mut den) = (None, None, None, None);
    for field in fields {
        if let Some(v) = field.strip_prefix('W') {
            width = v.parse::<u16>().ok();
        } else if let Some(v) = field.strip_prefix('H') {
            height = v.parse::<u16>().ok();
        } else if let Some(v) = field.strip_prefix('F') {
            if let Some((a, b)) = v.split_once(':') {
                num = a.parse::<u16>().ok();
                den = b.parse::<u16>().ok();
            }
        } else if let Some(v) = field.strip_prefix('C') {
            if !matches!(v, "420" | "420jpeg" | "420mpeg2" | "420paldv") {
                return Err(StreamEncodeError::Invalid(
                    "Only 8-bit YUV420 input is supported.",
                ));
            }
        } else if let Some(v) = field.strip_prefix('I') {
            if v != "p" && v != "?" {
                return Err(StreamEncodeError::Invalid(
                    "Interlaced input is unsupported.",
                ));
            }
        }
    }
    let (w, h, n, d) = (
        width.ok_or(StreamEncodeError::Invalid("Missing width."))?,
        height.ok_or(StreamEncodeError::Invalid("Missing height."))?,
        num.ok_or(StreamEncodeError::Invalid("Missing frame rate."))?,
        den.ok_or(StreamEncodeError::Invalid("Missing frame rate."))?,
    );
    if w == 0
        || h == 0
        || w > 800
        || h > 720
        || n == 0
        || d == 0
        || u32::from(n) > 30 * u32::from(d)
    {
        return Err(StreamEncodeError::Invalid(
            "Unsupported dimensions or frame rate.",
        ));
    }
    let (cw, ch) = ((u32::from(w) + 15) & !15, (u32::from(h) + 15) & !15);
    let mb = cw / 16 * (ch / 16);
    let levels = [
        (400, 240, 375, 11250, 96000usize, 200000usize),
        (800, 464, 1450, 43500, 256000, 400000),
        (800, 720, 2250, 67500, 384000, 600000),
    ];
    let (index, limits) = levels
        .iter()
        .enumerate()
        .find(|(_, l)| {
            cw <= l.0
                && ch <= l.1
                && mb <= l.2
                && u64::from(mb) * u64::from(n) <= u64::from(l.3 as u32) * u64::from(d)
        })
        .ok_or(StreamEncodeError::Invalid("No TVC level fits input."))?;
    let level = (index + 1) as u8;
    let start = output.stream_position()?;
    output.write_all(b"TVC1")?;
    output.write_all(&w.to_le_bytes())?;
    output.write_all(&h.to_le_bytes())?;
    output.write_all(&n.to_le_bytes())?;
    output.write_all(&d.to_le_bytes())?;
    output.write_all(&0u32.to_le_bytes())?;
    output.write_all(&[level, 0, 0, 0])?;
    let mut window = BRateWindow {
        window: usize::from(n).div_ceil(usize::from(d)),
        sizes: std::collections::VecDeque::new(),
        sum: 0,
        max_picture: if cfg!(feature = "safe-rate") {
            limits
                .4
                .min(limits.5 / usize::from(n).div_ceil(usize::from(d)))
        } else {
            limits.4
        },
        max_rate: limits.5,
    };
    let first = read_b_frame(&mut input, w, h)?
        .ok_or(StreamEncodeError::Invalid("Y4M stream has no frames."))?;
    let first_anchor = choose_b_anchor(&first, None, qp, &window, true)?;
    window.write(&mut output, &first_anchor.picture)?;
    let mut previous = first_anchor.reconstructed;
    let (mut frames, mut b_pictures, mut reencoded) = (1u32, 0u32, u32::from(first_anchor.retried));
    let mut last_key = 0u32;
    while let Some(middle) = read_b_frame(&mut input, w, h)? {
        if frames >= crate::MAX_FRAMES {
            return Err(StreamEncodeError::Invalid("Too many frames."));
        }
        let Some(future) = read_b_frame(&mut input, w, h)? else {
            let anchor = choose_b_anchor(
                &middle,
                Some(&previous),
                qp,
                &window,
                frames - last_key >= 96,
            )?;
            window.write(&mut output, &anchor.picture)?;
            frames += 1;
            reencoded += u32::from(anchor.retried);
            break;
        };
        if frames > crate::MAX_FRAMES - 2 {
            return Err(StreamEncodeError::Invalid("Too many frames."));
        }
        // Build the display-order I/P alternative against a cloned rate window.
        let middle_anchor = choose_b_anchor(
            &middle,
            Some(&previous),
            qp,
            &window,
            frames - last_key >= 96,
        )?;
        let mut trial = window.clone();
        trial.write(&mut std::io::sink(), &middle_anchor.picture)?;
        let baseline_key = if middle_anchor.picture[0] == 0 {
            frames
        } else {
            last_key
        };
        let next_anchor = choose_b_anchor(
            &future,
            Some(&middle_anchor.reconstructed),
            qp,
            &trial,
            frames + 1 - baseline_key >= 96,
        )?;
        let baseline_size = middle_anchor.picture.len() + next_anchor.picture.len();
        let baseline_error = squared_error(&middle, &middle_anchor.reconstructed)
            + squared_error(&future, &next_anchor.reconstructed);
        let mut group = None;
        // If both display-order pictures are exactly the standard skip coding,
        // their reconstructed anchors are identical. The corresponding B skip
        // has the same payload length, so it cannot beat the strict size guard.
        let skip = encode_skip_picture(usize::from(w), usize::from(h), qp);
        let identical_skip_pair = middle_anchor.picture == skip && next_anchor.picture == skip;
        if !identical_skip_pair && usize::from(w) % 16 == 0 && usize::from(h) % 16 == 0 {
            // Reuse the already encoded future anchor only when the middle
            // skip preserves its reference and neither window can constrain
            // any legal picture candidate. This keeps decisions unchanged.
            let can_reuse = middle_anchor.picture == skip
                && window.sum + skip.len() + window.max_picture <= window.max_rate;
            let proposed = if can_reuse {
                Ok(next_anchor.clone())
            } else {
                choose_b_anchor(
                    &future,
                    Some(&previous),
                    qp,
                    &window,
                    frames + 1 - last_key >= 96,
                )
            };
            if let Ok(anchor) = proposed {
                if let Ok(Some(b)) = try_b_between(&previous, &middle, &anchor.reconstructed, qp) {
                    if anchor.picture.len() + b.len() < baseline_size
                        && window.accepts(&[&anchor.picture, &b])
                    {
                        let left = super::frame::CodedPicture::from_aligned_frame(&previous)
                            .map_err(|_| {
                                StreamEncodeError::Invalid("Invalid previous B reference.")
                            })?;
                        let right =
                            super::frame::CodedPicture::from_aligned_frame(&anchor.reconstructed)
                                .map_err(|_| {
                                StreamEncodeError::Invalid("Invalid future B reference.")
                            })?;
                        let middle_recon =
                            super::frame::decode_picture(&b, w, h, [Some(&left), Some(&right)])
                                .map_err(|_| {
                                    StreamEncodeError::Invalid(
                                        "B picture could not be reconstructed.",
                                    )
                                })?
                                .display_frame();
                        let count = (middle.y.len() + middle.cb.len() + middle.cr.len()) as u64;
                        let group_error = squared_error(&middle, &middle_recon)
                            + squared_error(&future, &anchor.reconstructed);
                        if group_error <= baseline_error + count * 2 {
                            group = Some((anchor, b));
                        }
                    }
                }
            }
        }
        if let Some((anchor, b)) = group {
            window.write(&mut output, &anchor.picture)?;
            window.write(&mut output, &b)?;
            if anchor.picture[0] == 0 {
                last_key = frames + 1;
            }
            reencoded += u32::from(anchor.retried);
            previous = anchor.reconstructed;
            b_pictures += 1;
        } else {
            window.write(&mut output, &middle_anchor.picture)?;
            if middle_anchor.picture[0] == 0 {
                last_key = frames;
            }
            reencoded += u32::from(middle_anchor.retried);
            window.write(&mut output, &next_anchor.picture)?;
            if next_anchor.picture[0] == 0 {
                last_key = frames + 1;
            }
            reencoded += u32::from(next_anchor.retried);
            previous = next_anchor.reconstructed;
        }
        frames += 2;
    }
    if frames == 0 {
        return Err(StreamEncodeError::Invalid("Y4M stream has no frames."));
    }
    let end = output.stream_position()?;
    output.seek(SeekFrom::Start(start + 12))?;
    output.write_all(&frames.to_le_bytes())?;
    output.seek(SeekFrom::Start(end))?;
    Ok(BStreamSummary {
        frames,
        level,
        b_pictures,
        reencoded,
    })
}

#[cfg(all(test, feature = "b-pictures"))]
mod b_rate_tests {
    use super::BRateWindow;
    #[test]
    fn group_rate_probe_does_not_mutate_window() {
        let mut w = BRateWindow {
            window: 2,
            sizes: std::collections::VecDeque::new(),
            sum: 0,
            max_picture: 10,
            max_rate: 12,
        };
        let mut out = Vec::new();
        w.write(&mut out, &[0; 6]).unwrap();
        w.write(&mut out, &[0; 6]).unwrap();
        assert!(!w.accepts(&[&[0; 7]]));
        assert!(w.accepts(&[&[0; 5], &[0; 7]]));
        assert_eq!(w.sum, 12);
        w.write(&mut out, &[0; 5]).unwrap();
        assert_eq!(w.sum, 11);
        assert!(!w.accepts(&[&[0; 11]]));
    }
}

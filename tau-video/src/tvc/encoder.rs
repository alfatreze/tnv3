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
    mut input: R,
    mut output: W,
    qp: u8,
) -> Result<EncodeSummary, StreamEncodeError> {
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
        let use_prediction = frames % 96 != 0
            && reference
                .as_ref()
                .is_some_and(|r| close_to_reference(&frame, r));
        let mut q = qp;
        let picture = if use_prediction {
            let p = encode_skip_picture(wz, hz, qp);
            if p.len() > limits.4 || sum + p.len() > limits.5 {
                return Err(StreamEncodeError::Invalid(
                    "Predictive picture exceeds TVC level limits.",
                ));
            }
            predicted += 1;
            p
        } else {
            let p = loop {
                match encode_intra_picture(&frame, q) {
                    Ok(p) if p.len() <= limits.4 && sum + p.len() <= limits.5 => break p,
                    _ if q < 51 => q += 1,
                    _ => {
                        return Err(StreamEncodeError::Invalid(
                            "Picture cannot fit TVC level limits.",
                        ));
                    }
                }
            };
            reference = Some(super::frame::decode_intra_picture(&p, w, h).map_err(|_| {
                StreamEncodeError::Invalid("Encoded picture could not be reconstructed.")
            })?);
            p
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

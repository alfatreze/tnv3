//! Adaptive TVC coefficient block entropy coding, draft 5.
use super::range::{Probability, RangeDecoder, RangeEncoder};
use std::{error, fmt};
const PCLASS8: [usize; 64] = [
    0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 10, 10, 10, 10, 10, 10, 10, 10, 10,
    10, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
    12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
];
const PCLASS4: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8];
/// Coefficient context class and block size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockClass {
    /// 8×8 luma.
    Luma8,
    /// 8×8 chroma.
    Chroma8,
    /// 4×4 luma.
    Luma4,
}
impl BlockClass {
    fn params(self) -> (usize, usize, usize) {
        match self {
            Self::Luma8 => (0, 64, 6),
            Self::Chroma8 => (1, 64, 6),
            Self::Luma4 => (2, 16, 4),
        }
    }
}
/// Invalid or out-of-range coefficient syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoefficientError;
impl fmt::Display for CoefficientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC coefficient block.")
    }
}
impl error::Error for CoefficientError {}
/// Adaptive contexts shared across blocks within one slice.
#[derive(Clone, Debug)]
pub struct CoefficientContexts {
    last: [[Probability; 64]; 3],
    sig: [[[Probability; 3]; 13]; 3],
    gt1: [[[Probability; 3]; 3]; 3],
    lrem: [[Probability; 6]; 3],
}
impl Default for CoefficientContexts {
    fn default() -> Self {
        let p = Probability::default();
        Self {
            last: [[p; 64]; 3],
            sig: [[[p; 3]; 13]; 3],
            gt1: [[[p; 3]; 3]; 3],
            lrem: [[p; 6]; 3],
        }
    }
}
fn eg_encode(r: &mut RangeEncoder, c: &mut [Probability; 6], value: u32) {
    let mut v = value;
    let mut k = 0;
    let mut i = 0;
    while v >= 1u32 << k {
        r.bit(&mut c[i.min(5)], true);
        v -= 1u32 << k;
        k += 1;
        i += 1;
    }
    r.bit(&mut c[i.min(5)], false);
    for b in (0..k).rev() {
        r.bypass(v & (1 << b) != 0);
    }
}
fn eg_decode(r: &mut RangeDecoder<'_>, c: &mut [Probability; 6]) -> Result<u32, CoefficientError> {
    let mut base = 0u32;
    let mut k = 0usize;
    while r.bit(&mut c[k.min(5)]) {
        base += 1u32 << k;
        k += 1;
        if k > 24 {
            return Err(CoefficientError);
        }
    }
    let mut suffix = 0u32;
    for _ in 0..k {
        suffix = (suffix << 1) | u32::from(r.bypass());
    }
    Ok(base + suffix)
}
/// Encode one nonzero coefficient block. Contexts must persist across a slice.
/// The coded-block flag is handled separately by the macroblock layer.
pub fn encode_block(
    r: &mut RangeEncoder,
    c: &mut CoefficientContexts,
    class: BlockClass,
    levels: &[i16],
) -> Result<(), CoefficientError> {
    let (cc, n, bits) = class.params();
    if levels.len() != n {
        return Err(CoefficientError);
    }
    let last = levels
        .iter()
        .rposition(|&x| x != 0)
        .ok_or(CoefficientError)?;
    let mut node = 1usize;
    for b in (0..bits).rev() {
        let bit = (last >> b) & 1 != 0;
        r.bit(&mut c.last[cc][node], bit);
        node = node * 2 + usize::from(bit);
    }
    let mut ngt = 0usize;
    for i in (0..=last).rev() {
        let value = levels[i];
        if i < last {
            let near =
                usize::from(levels[i + 1] != 0) + usize::from(i + 2 <= last && levels[i + 2] != 0);
            let pc = if n == 64 { PCLASS8[i] } else { PCLASS4[i] };
            r.bit(&mut c.sig[cc][pc][near], value != 0);
            if value == 0 {
                continue;
            }
        }
        let magnitude = i32::from(value).unsigned_abs();
        let gc = if n == 64 {
            if i < 6 {
                0
            } else if i < 20 {
                1
            } else {
                2
            }
        } else if i < 3 {
            0
        } else if i < 8 {
            1
        } else {
            2
        };
        let gt = magnitude > 1;
        r.bit(&mut c.gt1[cc][gc][ngt.min(2)], gt);
        if gt {
            eg_encode(r, &mut c.lrem[cc], magnitude - 2);
            ngt += 1;
        }
        r.bypass(value < 0);
    }
    Ok(())
}
/// Decode a coefficient block into a caller-provided 16- or 64-element buffer.
/// Malformed magnitudes and Exp-Golomb values return an error.
pub fn decode_block(
    r: &mut RangeDecoder<'_>,
    c: &mut CoefficientContexts,
    class: BlockClass,
    levels: &mut [i16],
) -> Result<(), CoefficientError> {
    let (cc, n, bits) = class.params();
    if levels.len() != n {
        return Err(CoefficientError);
    }
    levels.fill(0);
    let mut node = 1usize;
    for _ in 0..bits {
        node = node * 2 + usize::from(r.bit(&mut c.last[cc][node]));
    }
    let last = node - n;
    let mut ngt = 0usize;
    for i in (0..=last).rev() {
        if i < last {
            let near =
                usize::from(levels[i + 1] != 0) + usize::from(i + 2 <= last && levels[i + 2] != 0);
            let pc = if n == 64 { PCLASS8[i] } else { PCLASS4[i] };
            if !r.bit(&mut c.sig[cc][pc][near]) {
                continue;
            }
        }
        let gc = if n == 64 {
            if i < 6 {
                0
            } else if i < 20 {
                1
            } else {
                2
            }
        } else if i < 3 {
            0
        } else if i < 8 {
            1
        } else {
            2
        };
        let gt = r.bit(&mut c.gt1[cc][gc][ngt.min(2)]);
        let magnitude = if gt {
            ngt += 1;
            2 + eg_decode(r, &mut c.lrem[cc])?
        } else {
            1
        };
        let negative = r.bypass();
        if magnitude > 32768 || (!negative && magnitude > 32767) {
            return Err(CoefficientError);
        }
        levels[i] = if negative {
            -(magnitude as i32) as i16
        } else {
            magnitude as i16
        };
    }
    Ok(())
}

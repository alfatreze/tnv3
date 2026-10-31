//! Signed adaptive Exp-Golomb syntax for TVC motion and quantizer differences.
use super::range::{Probability, RangeDecoder, RangeEncoder};
use std::{error, fmt};
/// Invalid context array or signed Exp-Golomb value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedCodeError;
impl fmt::Display for SignedCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC signed Exp-Golomb value.")
    }
}
impl error::Error for SignedCodeError {}
/// Largest representable magnitude with the format's 24-bit prefix bound.
pub const MAX_MAGNITUDE: i32 = 33_554_431;
/// Encode one signed value, updating the supplied contexts.
pub fn encode_signed(
    r: &mut RangeEncoder,
    zero: &mut Probability,
    ctx: &mut [Probability],
    value: i32,
) -> Result<(), SignedCodeError> {
    if ctx.is_empty() || value.unsigned_abs() > MAX_MAGNITUDE as u32 {
        return Err(SignedCodeError);
    }
    r.bit(zero, value != 0);
    if value == 0 {
        return Ok(());
    }
    let mut remaining = value.unsigned_abs() - 1;
    let mut k = 0usize;
    while remaining >= 1u32 << k {
        r.bit(&mut ctx[k.min(ctx.len() - 1)], true);
        remaining -= 1u32 << k;
        k += 1;
    }
    r.bit(&mut ctx[k.min(ctx.len() - 1)], false);
    for b in (0..k).rev() {
        r.bypass(remaining & (1u32 << b) != 0);
    }
    r.bypass(value < 0);
    Ok(())
}
/// Decode one signed value, rejecting out-of-range Exp-Golomb prefixes.
pub fn decode_signed(
    r: &mut RangeDecoder<'_>,
    zero: &mut Probability,
    ctx: &mut [Probability],
) -> Result<i32, SignedCodeError> {
    if ctx.is_empty() {
        return Err(SignedCodeError);
    }
    if !r.bit(zero) {
        return Ok(0);
    }
    let mut base = 0u32;
    let mut k = 0usize;
    while r.bit(&mut ctx[k.min(ctx.len() - 1)]) {
        base += 1u32 << k;
        k += 1;
        if k > 24 {
            return Err(SignedCodeError);
        }
    }
    let mut suffix = 0u32;
    for _ in 0..k {
        suffix = (suffix << 1) | u32::from(r.bypass());
    }
    let magnitude = (1 + base + suffix) as i32;
    Ok(if r.bypass() { -magnitude } else { magnitude })
}

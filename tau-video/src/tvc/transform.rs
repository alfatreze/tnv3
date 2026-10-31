//! Integer dequantization and inverse transforms for TVC draft 5.
use std::{error, fmt};
pub(super) const QS: [i64; 52] = [
    10, 11, 13, 14, 16, 18, 20, 23, 25, 29, 32, 36, 40, 45, 51, 57, 64, 72, 81, 91, 102, 114, 128,
    144, 161, 181, 203, 228, 256, 287, 323, 362, 406, 456, 512, 575, 645, 724, 813, 912, 1024,
    1149, 1290, 1448, 1625, 1825, 2048, 2299, 2580, 2896, 3251, 3649,
];
const T8: [[i64; 8]; 8] = [
    [64, 64, 64, 64, 64, 64, 64, 64],
    [89, 75, 50, 18, -18, -50, -75, -89],
    [84, 35, -35, -84, -84, -35, 35, 84],
    [75, -18, -89, -50, 50, 89, 18, -75],
    [64, -64, -64, 64, 64, -64, -64, 64],
    [50, -89, 18, 75, -75, -18, 89, -50],
    [35, -84, 84, -35, -35, 84, -84, 35],
    [18, -50, 75, -89, 89, -75, 50, -18],
];
const T4: [[i64; 4]; 4] = [
    [64, 64, 64, 64],
    [84, 35, -35, -84],
    [64, -64, -64, 64],
    [35, -84, 84, -35],
];
/// An invalid quantizer was supplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidQuantizer;
impl fmt::Display for InvalidQuantizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TVC quantizer must be in 0..=51.")
    }
}
impl error::Error for InvalidQuantizer {}
fn transform<const N: usize>(
    levels: &[i16],
    qp: u8,
    t: &[[i64; N]; N],
    second_shift: u32,
) -> Result<[[i32; N]; N], InvalidQuantizer> {
    let step = *QS.get(usize::from(qp)).ok_or(InvalidQuantizer)?;
    let mut c = [[0i64; N]; N];
    let mut idx = 0;
    for sum in 0..(2 * N - 1) {
        let lo = sum.saturating_sub(N - 1);
        let hi = sum.min(N - 1);
        for offset in 0..=hi - lo {
            let row = if sum % 2 == 1 {
                lo + offset
            } else {
                hi - offset
            };
            let col = sum - row;
            let level = i64::from(levels[idx]);
            let magnitude = ((level.abs() * step + 8) >> 4).min(32767);
            c[row][col] = if level < 0 { -magnitude } else { magnitude };
            idx += 1;
        }
    }
    let mut e = [[0i64; N]; N];
    for y in 0..N {
        for v in 0..N {
            let mut s = 0;
            for u in 0..N {
                s += t[u][y] * c[u][v];
            }
            e[y][v] = ((s + 64) >> 7).clamp(-32768, 32767);
        }
    }
    let mut out = [[0i32; N]; N];
    for y in 0..N {
        for x in 0..N {
            let mut s = 0;
            for v in 0..N {
                s += e[y][v] * t[v][x];
            }
            out[y][x] = ((s + (1 << (second_shift - 1))) >> second_shift) as i32;
        }
    }
    Ok(out)
}
/// Reconstruct an 8×8 residual from quantized coefficients in TVC scan order.
pub fn inverse_8x8(levels: &[i16; 64], qp: u8) -> Result<[[i32; 8]; 8], InvalidQuantizer> {
    transform(levels, qp, &T8, 8)
}
/// Reconstruct a 4×4 luma residual from quantized coefficients in TVC scan order.
pub fn inverse_4x4(levels: &[i16; 16], qp: u8) -> Result<[[i32; 4]; 4], InvalidQuantizer> {
    transform(levels, qp, &T4, 7)
}

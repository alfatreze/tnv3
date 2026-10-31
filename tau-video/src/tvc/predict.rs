//! Intra-picture prediction for TVC 8×8 blocks.
/// TVC intra-prediction mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntraMode {
    /// Constant mean of available neighbors.
    Dc,
    /// Replicate the top edge vertically.
    Vertical,
    /// Replicate the left edge horizontally.
    Horizontal,
    /// Interpolate toward the far neighboring samples.
    Planar,
}
/// Predict an 8×8 block from its top and left edges. A missing edge is `None`.
/// Slice boundaries make the top edge unavailable even when a prior row exists.
pub fn intra_8x8(mode: IntraMode, top: Option<&[u8; 8]>, left: Option<&[u8; 8]>) -> [[u8; 8]; 8] {
    if mode == IntraMode::Dc {
        let sum: u32 = top
            .into_iter()
            .flatten()
            .chain(left.into_iter().flatten())
            .map(|&x| u32::from(x))
            .sum();
        let dc = match (top.is_some(), left.is_some()) {
            (true, true) => (sum + 8) >> 4,
            (true, false) | (false, true) => (sum + 4) >> 3,
            (false, false) => 128,
        };
        return [[dc as u8; 8]; 8];
    }
    let t = top
        .copied()
        .unwrap_or_else(|| [left.map_or(128, |l| l[0]); 8]);
    let l = left
        .copied()
        .unwrap_or_else(|| [top.map_or(128, |t| t[0]); 8]);
    let mut out = [[0u8; 8]; 8];
    for y in 0..8 {
        for x in 0..8 {
            out[y][x] = match mode {
                IntraMode::Vertical => t[x],
                IntraMode::Horizontal => l[y],
                IntraMode::Planar => {
                    (((7 - x) * usize::from(l[y])
                        + (x + 1) * usize::from(t[7])
                        + (7 - y) * usize::from(t[x])
                        + (y + 1) * usize::from(l[7])
                        + 8)
                        >> 4) as u8
                }
                IntraMode::Dc => 128,
            };
        }
    }
    out
}

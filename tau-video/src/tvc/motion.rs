//! Motion-vector prediction for TVC macroblock syntax.
use super::deblock::DeblockMacroblock;
use std::{error, fmt};
/// An invalid macroblock grid, position, slice boundary, or reference list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMotionGrid;
impl fmt::Display for InvalidMotionGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC motion-prediction grid.")
    }
}
impl error::Error for InvalidMotionGrid {}
/// Predict a motion vector from previously decoded macroblocks in raster order.
///
/// `slice_start` is the first macroblock row of the current slice; neighbors
/// from preceding slices are unavailable. `grid` contains full-grid entries,
/// but only previously decoded neighboring entries are read. `list` is 0 or 1.
pub fn predict_motion(
    grid: &[DeblockMacroblock],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    slice_start: usize,
    list: usize,
) -> Result<[i16; 2], InvalidMotionGrid> {
    if width == 0
        || height == 0
        || width > 50
        || height > 45
        || grid.len() != width * height
        || x >= width
        || y >= height
        || slice_start > y
        || list > 1
    {
        return Err(InvalidMotionGrid);
    }
    let left = if x > 0 {
        Some(&grid[y * width + x - 1])
    } else {
        None
    };
    let above = if y > slice_start {
        Some(&grid[(y - 1) * width + x])
    } else {
        None
    };
    let above_right = if y > slice_start && x + 1 < width {
        Some(&grid[(y - 1) * width + x + 1])
    } else {
        None
    };
    let above_left = if y > slice_start && x > 0 {
        Some(&grid[(y - 1) * width + x - 1])
    } else {
        None
    };
    fn vector(m: Option<&DeblockMacroblock>, list: usize) -> [i16; 2] {
        m.filter(|m| m.uses & (1 << list) != 0)
            .map_or([0, 0], |m| m.motion[list])
    }
    let a = vector(left, list);
    let Some(b) = above else {
        return Ok(a);
    };
    let b = vector(Some(b), list);
    let c = vector(above_right.or(above_left), list);
    let mut out = [0; 2];
    for k in 0..2 {
        let mut values = [a[k], b[k], c[k]];
        values.sort_unstable();
        out[k] = values[1];
    }
    Ok(out)
}

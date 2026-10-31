//! TVC motion-compensated inter prediction.
use std::{error, fmt};
/// Interpolation used for one reference plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Half-sample bilinear interpolation (also used for half-pel chroma).
    HalfSample,
    /// Quarter-sample luma interpolation using the TVC four-tap filters.
    QuarterLuma,
    /// Eighth-sample chroma interpolation using a quarter-luma vector.
    EighthChroma,
}
/// Invalid reference-plane layout or prediction-block size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPlane;
impl fmt::Display for InvalidPlane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC reference plane or block dimensions.")
    }
}
impl error::Error for InvalidPlane {}
/// A strided reference plane; only the declared dimensions are accessible.
pub struct ReferencePlane<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    stride: usize,
}
impl<'a> ReferencePlane<'a> {
    /// Validate the plane's dimensions and backing memory.
    pub fn new(
        data: &'a [u8],
        width: usize,
        height: usize,
        stride: usize,
    ) -> Result<Self, InvalidPlane> {
        if width == 0 || height == 0 || width > 800 || height > 720 || stride < width {
            return Err(InvalidPlane);
        }
        let size = (height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(width))
            .ok_or(InvalidPlane)?;
        if size > data.len() {
            return Err(InvalidPlane);
        }
        Ok(Self {
            data,
            width,
            height,
            stride,
        })
    }
    fn pixel(&self, x: i64, y: i64) -> i64 {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        i64::from(self.data[y * self.stride + x])
    }
    /// Predict a block no larger than 16×16 into a fixed-size row-major array.
    /// `origin` is in plane pixels and `motion` uses the units of `interpolation`.
    /// The returned array has a stride of 16; only `size` pixels are meaningful.
    pub fn predict(
        &self,
        origin: (i32, i32),
        motion: (i32, i32),
        size: (u8, u8),
        interpolation: Interpolation,
    ) -> Result<[u8; 256], InvalidPlane> {
        let (bw, bh) = size;
        if bw == 0 || bh == 0 || bw > 16 || bh > 16 {
            return Err(InvalidPlane);
        }
        let (shift, mask) = match interpolation {
            Interpolation::HalfSample => (1, 1),
            Interpolation::QuarterLuma => (2, 3),
            Interpolation::EighthChroma => (3, 7),
        };
        let (mx, my) = (i64::from(motion.0), i64::from(motion.1));
        let (ix, iy) = (mx >> shift, my >> shift);
        let (fx, fy) = (mx & mask, my & mask);
        let mut out = [0u8; 256];
        const F: [[i64; 4]; 4] = [
            [0, 64, 0, 0],
            [-4, 56, 14, -2],
            [-4, 36, 36, -4],
            [-2, 14, 56, -4],
        ];
        for y in 0..usize::from(bh) {
            for x in 0..usize::from(bw) {
                let px = i64::from(origin.0) + x as i64 + ix;
                let py = i64::from(origin.1) + y as i64 + iy;
                let v = match interpolation {
                    Interpolation::HalfSample => {
                        let a = self.pixel(px, py);
                        match (fx, fy) {
                            (0, 0) => a,
                            (1, 0) => (a + self.pixel(px + 1, py) + 1) >> 1,
                            (0, 1) => (a + self.pixel(px, py + 1) + 1) >> 1,
                            _ => {
                                (a + self.pixel(px + 1, py)
                                    + self.pixel(px, py + 1)
                                    + self.pixel(px + 1, py + 1)
                                    + 2)
                                    >> 2
                            }
                        }
                    }
                    Interpolation::EighthChroma => {
                        ((8 - fx) * (8 - fy) * self.pixel(px, py)
                            + fx * (8 - fy) * self.pixel(px + 1, py)
                            + (8 - fx) * fy * self.pixel(px, py + 1)
                            + fx * fy * self.pixel(px + 1, py + 1)
                            + 32)
                            >> 6
                    }
                    Interpolation::QuarterLuma => {
                        let xf = F[fx as usize];
                        let yf = F[fy as usize];
                        if fx == 0 && fy == 0 {
                            self.pixel(px, py)
                        } else if fy == 0 {
                            (xf.iter()
                                .enumerate()
                                .map(|(k, &w)| w * self.pixel(px + k as i64 - 1, py))
                                .sum::<i64>()
                                + 32)
                                >> 6
                        } else if fx == 0 {
                            (yf.iter()
                                .enumerate()
                                .map(|(k, &w)| w * self.pixel(px, py + k as i64 - 1))
                                .sum::<i64>()
                                + 32)
                                >> 6
                        } else {
                            let mut s = 0;
                            for (j, &wy) in yf.iter().enumerate() {
                                let mut t = 0;
                                for (k, &wx) in xf.iter().enumerate() {
                                    t += wx * self.pixel(px + k as i64 - 1, py + j as i64 - 1);
                                }
                                s += wy * t;
                            }
                            (s + 2048) >> 12
                        }
                    }
                };
                out[y * 16 + x] = v.clamp(0, 255) as u8;
            }
        }
        Ok(out)
    }
}
/// Average two single-list predictions using TVC's rounded biprediction rule.
pub fn average(a: u8, b: u8) -> u8 {
    ((u16::from(a) + u16::from(b) + 1) >> 1) as u8
}

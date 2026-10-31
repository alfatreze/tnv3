//! Decoding TVC macroblock syntax for one picture slice.
use super::{
    coeff::{BlockClass, CoefficientContexts, decode_block},
    picture::PictureType,
    range::{Probability, RangeDecoder},
    signed::decode_signed,
};
use std::{error, fmt};
/// Macroblock prediction category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MacroblockKind {
    /// Intra prediction.
    #[default]
    Intra,
    /// Motion-compensated prediction with residual.
    Inter,
    /// Motion-compensated prediction without residual.
    Skip,
}
/// A decoded macroblock and its quantized coefficients in scan order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntraMacroblock {
    /// Macroblock prediction category.
    pub kind: MacroblockKind,
    /// Bit mask of used reference lists.
    pub uses: u8,
    /// Motion vectors for the two reference lists.
    pub motion: [[i16; 2]; 2],
    /// Motion-vector differences for context prediction.
    pub motion_difference: [[i16; 2]; 2],
    /// Luma prediction modes, in 8×8 block order.
    pub luma_modes: [u8; 4],
    /// Chroma prediction mode.
    pub chroma_mode: u8,
    /// Macroblock quantizer.
    pub qp: u8,
    /// Six-bit coded-block pattern.
    pub coded: u8,
    /// Four-bit mask selecting 4×4 luma transforms.
    pub transform_4x4: u8,
    /// Sixteen-bit mask of coded 4×4 sub-blocks.
    pub subblocks_4x4: u16,
    /// Quantized coefficients; 4×4 sub-blocks occupy four consecutive groups of 16.
    pub coefficients: [[i16; 64]; 6],
}
#[derive(Clone, Copy, Default)]
struct Neighbor {
    kind: MacroblockKind,
    uses: u8,
    motion: [[i16; 2]; 2],
    mvd: [[i16; 2]; 2],
    modes: [u8; 4],
    coded: u8,
    t4: u8,
}
/// Invalid dimensions, quantizer, or entropy-coded macroblock data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntraDecodeError;
impl fmt::Display for IntraDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid TVC macroblock syntax.")
    }
}
impl error::Error for IntraDecodeError {}
/// Decodes macroblocks in raster order within a single picture slice.
///
/// This decodes syntax only; prediction, reconstruction and deblocking are
/// separate steps. Like the format, range input is zero-padded beyond its end.
pub struct IntraSliceDecoder<'a> {
    input: RangeDecoder<'a>,
    failed: bool,
    width: usize,
    rows: usize,
    next: usize,
    grid: Vec<Neighbor>,
    picture_type: PictureType,
    skip: [Probability; 3],
    intra: [Probability; 3],
    bdir: [Probability; 2],
    mvz: [[Probability; 3]; 2],
    mvm: [[Probability; 6]; 2],
    picture_qp: u8,
    prev_qp: u8,
    prev_dq: i32,
    aq: bool,
    t4ok: bool,
    mpm: [Probability; 1],
    rem: [Probability; 2],
    cmode: [Probability; 3],
    cbp: [[Probability; 3]; 3],
    dqz: [Probability; 2],
    dqm: [Probability; 3],
    t4: [Probability; 3],
    cbf4: [Probability; 4],
    coeff: CoefficientContexts,
}
impl<'a> IntraSliceDecoder<'a> {
    /// Create a new independent slice, with contexts reset to their initial values.
    pub fn new(
        data: &'a [u8],
        width: usize,
        rows: usize,
        qp: u8,
        aq: bool,
        t4ok: bool,
    ) -> Result<Self, IntraDecodeError> {
        if width == 0 || width > 50 || rows == 0 || rows > 45 || qp > 51 {
            return Err(IntraDecodeError);
        }
        let p = Probability::default();
        Ok(Self {
            input: RangeDecoder::new(data),
            failed: false,
            width,
            rows,
            next: 0,
            grid: vec![Neighbor::default(); width * rows],
            picture_type: PictureType::Intra,
            skip: [p; 3],
            intra: [p; 3],
            bdir: [p; 2],
            mvz: [[p; 3]; 2],
            mvm: [[p; 6]; 2],
            picture_qp: qp,
            prev_qp: qp,
            prev_dq: 0,
            aq,
            t4ok,
            mpm: [p; 1],
            rem: [p; 2],
            cmode: [p; 3],
            cbp: [[p; 3]; 3],
            dqz: [p; 2],
            dqm: [p; 3],
            t4: [p; 3],
            cbf4: [p; 4],
            coeff: CoefficientContexts::default(),
        })
    }
    /// Construct a decoder for intra, predictive or bidirectional picture syntax.
    pub fn for_picture(
        data: &'a [u8],
        width: usize,
        rows: usize,
        qp: u8,
        aq: bool,
        t4ok: bool,
        kind: PictureType,
    ) -> Result<Self, IntraDecodeError> {
        let mut result = Self::new(data, width, rows, qp, aq, t4ok)?;
        result.picture_type = kind;
        Ok(result)
    }
    /// Decode the next macroblock; returns `None` after the declared slice size.
    pub fn next_macroblock(&mut self) -> Result<Option<IntraMacroblock>, IntraDecodeError> {
        if self.failed {
            return Err(IntraDecodeError);
        }
        let result = self.decode_next();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn decode_next(&mut self) -> Result<Option<IntraMacroblock>, IntraDecodeError> {
        if self.next == self.width * self.rows {
            return Ok(None);
        }
        let x = self.next % self.width;
        let y = self.next / self.width;
        let left = if x > 0 {
            Some(self.grid[self.next - 1])
        } else {
            None
        };
        let top = if y > 0 {
            Some(self.grid[self.next - self.width])
        } else {
            None
        };
        let ar = if y > 0 && x + 1 < self.width {
            Some(self.grid[self.next - self.width + 1])
        } else {
            None
        };
        let al = if y > 0 && x > 0 {
            Some(self.grid[self.next - self.width - 1])
        } else {
            None
        };
        let mut predicted = [[0i16; 2]; 2];
        for (list, pred) in predicted.iter_mut().enumerate() {
            let vector = |v: Option<Neighbor>| {
                v.filter(|v| v.uses & (1 << list) != 0)
                    .map_or([0, 0], |v| v.motion[list])
            };
            let a = vector(left);
            if let Some(t) = top {
                let b = vector(Some(t));
                let c = vector(ar.or(al));
                for k in 0..2 {
                    let mut v = [a[k], b[k], c[k]];
                    v.sort_unstable();
                    pred[k] = v[1];
                }
            } else {
                *pred = a;
            }
        }
        let mut kind = MacroblockKind::Intra;
        if self.picture_type != PictureType::Intra {
            let context = usize::from(left.is_some_and(|m| m.kind == MacroblockKind::Skip))
                + usize::from(top.is_some_and(|m| m.kind == MacroblockKind::Skip));
            if self.input.bit(&mut self.skip[context]) {
                let uses = if self.picture_type == PictureType::Bidirectional {
                    3
                } else {
                    1
                };
                let mut motion = [[0; 2]; 2];
                for list in 0..2 {
                    if uses & (1 << list) != 0 {
                        motion[list] = predicted[list];
                    }
                }
                self.grid[self.next] = Neighbor {
                    kind: MacroblockKind::Skip,
                    uses,
                    motion,
                    ..Neighbor::default()
                };
                self.next += 1;
                return Ok(Some(IntraMacroblock {
                    kind: MacroblockKind::Skip,
                    uses,
                    motion,
                    motion_difference: [[0; 2]; 2],
                    luma_modes: [0; 4],
                    chroma_mode: 0,
                    qp: self.prev_qp,
                    coded: 0,
                    transform_4x4: 0,
                    subblocks_4x4: 0,
                    coefficients: [[0; 64]; 6],
                }));
            }
            let context = usize::from(left.is_some_and(|m| m.kind == MacroblockKind::Intra))
                + usize::from(top.is_some_and(|m| m.kind == MacroblockKind::Intra));
            if !self.input.bit(&mut self.intra[context]) {
                kind = MacroblockKind::Inter;
            }
        }
        let qp = if self.aq {
            let d = decode_signed(
                &mut self.input,
                &mut self.dqz[usize::from(self.prev_dq != 0)],
                &mut self.dqm,
            )
            .map_err(|_| IntraDecodeError)?;
            let q = (i32::from(self.prev_qp) + d).clamp(0, 51) as u8;
            self.prev_qp = q;
            self.prev_dq = d;
            q
        } else {
            self.picture_qp
        };
        let mut modes = [0u8; 4];
        let mut cm = 0u8;
        let mut uses = 0u8;
        let mut motion = [[0i16; 2]; 2];
        let mut mvd = [[0i16; 2]; 2];
        if kind == MacroblockKind::Intra {
            for b in 0..4 {
                let l = if b & 1 != 0 {
                    modes[b - 1]
                } else {
                    left.filter(|m| m.kind == MacroblockKind::Intra)
                        .map_or(0, |m| m.modes[b + 1])
                };
                let t = if b & 2 != 0 {
                    modes[b - 2]
                } else {
                    top.filter(|m| m.kind == MacroblockKind::Intra)
                        .map_or(0, |m| m.modes[b + 2])
                };
                let predicted = l.min(t);
                modes[b] = if self.input.bit(&mut self.mpm[0]) {
                    predicted
                } else {
                    let a = self.input.bit(&mut self.rem[0]);
                    let rem = if a {
                        1 + usize::from(self.input.bit(&mut self.rem[1]))
                    } else {
                        0
                    };
                    let mut choices = [0u8; 3];
                    let mut i = 0;
                    for mode in 0..4 {
                        if mode != predicted {
                            choices[i] = mode;
                            i += 1;
                        }
                    }
                    choices[rem]
                };
            }
            cm = if !self.input.bit(&mut self.cmode[0]) {
                0
            } else if !self.input.bit(&mut self.cmode[1]) {
                1
            } else {
                2 + u8::from(self.input.bit(&mut self.cmode[2]))
            };
        } else {
            uses = if self.picture_type == PictureType::Bidirectional {
                if !self.input.bit(&mut self.bdir[0]) {
                    3
                } else if self.input.bit(&mut self.bdir[1]) {
                    2
                } else {
                    1
                }
            } else {
                1
            };
            for list in 0..2 {
                if uses & (1 << list) == 0 {
                    continue;
                }
                for component in 0..2 {
                    let neighbor_diff = |m: Option<Neighbor>| {
                        m.filter(|m| m.kind == MacroblockKind::Inter && m.uses & (1 << list) != 0)
                            .map_or(0, |m| i32::from(m.mvd[list][component]).abs())
                    };
                    let sum = neighbor_diff(left) + neighbor_diff(top);
                    let class = if sum < 2 {
                        0
                    } else if sum < 16 {
                        1
                    } else {
                        2
                    };
                    let diff = decode_signed(
                        &mut self.input,
                        &mut self.mvz[component][class],
                        &mut self.mvm[component],
                    )
                    .map_err(|_| IntraDecodeError)?;
                    mvd[list][component] = i16::try_from(diff).map_err(|_| IntraDecodeError)?;
                    motion[list][component] =
                        i16::try_from(i32::from(predicted[list][component]) + diff)
                            .map_err(|_| IntraDecodeError)?;
                }
            }
        }
        let mut coded = 0u8;
        for b in 0..6 {
            let (l, t, cl) = if b < 4 {
                let l = if b & 1 != 0 {
                    (coded >> (b - 1)) & 1
                } else {
                    left.map_or(0, |m| (m.coded >> (b + 1)) & 1)
                };
                let t = if b & 2 != 0 {
                    (coded >> (b - 2)) & 1
                } else {
                    top.map_or(0, |m| (m.coded >> (b + 2)) & 1)
                };
                (l, t, 0)
            } else {
                (
                    left.map_or(0, |m| (m.coded >> b) & 1),
                    top.map_or(0, |m| (m.coded >> b) & 1),
                    b - 3,
                )
            };
            if self.input.bit(&mut self.cbp[cl][usize::from(l + t)]) {
                coded |= 1 << b;
            }
        }
        let mut t4 = 0u8;
        let mut sub = 0u16;
        let mut levels = [[0i16; 64]; 6];
        for (b, block) in levels.iter_mut().enumerate() {
            if (coded >> b) & 1 == 0 {
                continue;
            }
            if b < 4 && self.t4ok {
                let l = if b & 1 != 0 {
                    (t4 >> (b - 1)) & 1
                } else {
                    left.map_or(0, |m| (m.t4 >> (b + 1)) & 1)
                };
                let t = if b & 2 != 0 {
                    (t4 >> (b - 2)) & 1
                } else {
                    top.map_or(0, |m| (m.t4 >> (b + 2)) & 1)
                };
                if self.input.bit(&mut self.t4[usize::from(l + t)]) {
                    t4 |= 1 << b;
                    for k in 0..4 {
                        if self.input.bit(&mut self.cbf4[k]) {
                            sub |= 1 << (4 * b + k);
                            decode_block(
                                &mut self.input,
                                &mut self.coeff,
                                BlockClass::Luma4,
                                &mut block[16 * k..16 * k + 16],
                            )
                            .map_err(|_| IntraDecodeError)?;
                        }
                    }
                    continue;
                }
            }
            let class = if b < 4 {
                BlockClass::Luma8
            } else {
                BlockClass::Chroma8
            };
            decode_block(&mut self.input, &mut self.coeff, class, block)
                .map_err(|_| IntraDecodeError)?;
        }
        self.grid[self.next] = Neighbor {
            modes,
            coded,
            t4,
            kind,
            uses,
            motion,
            mvd,
        };
        self.next += 1;
        Ok(Some(IntraMacroblock {
            kind,
            uses,
            motion,
            motion_difference: mvd,
            luma_modes: modes,
            chroma_mode: cm,
            qp,
            coded,
            transform_4x4: t4,
            subblocks_4x4: sub,
            coefficients: levels,
        }))
    }
}

//! TVC draft 5 binary range coding. Contexts are reset for each slice.

/// An adaptive probability, initially an equal probability of either bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probability(u16);
impl Default for Probability {
    fn default() -> Self {
        Self(16_384)
    }
}
impl Probability {
    fn update(&mut self, bit: bool) {
        let p = u32::from(self.0);
        self.0 = if bit {
            (p - (p >> 5)) as u16
        } else {
            (p + ((32768 - p) >> 5)) as u16
        };
    }
}

/// A TVC range decoder. The bitstream specifies zero bytes after slice end.
pub struct RangeDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
}
impl<'a> RangeDecoder<'a> {
    /// Initialize from one slice; the specification pads short slices with zero bytes.
    pub fn new(data: &'a [u8]) -> Self {
        let mut this = Self {
            data,
            pos: 0,
            range: u32::MAX,
            code: 0,
        };
        for _ in 0..5 {
            this.code = (this.code << 8) | u32::from(this.next_byte());
        }
        this
    }
    fn next_byte(&mut self) -> u8 {
        if let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            b
        } else {
            0
        }
    }
    fn normalize(&mut self) {
        while self.range < 1 << 24 {
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(self.next_byte());
        }
    }
    /// Decode one bin and update its adaptive context.
    pub fn bit(&mut self, p: &mut Probability) -> bool {
        let bound = (self.range >> 15) * u32::from(p.0);
        let b = self.code >= bound;
        if b {
            self.code -= bound;
            self.range -= bound;
        } else {
            self.range = bound;
        }
        p.update(b);
        self.normalize();
        b
    }
    /// Decode an equiprobable bypass bin.
    pub fn bypass(&mut self) -> bool {
        self.range >>= 1;
        let b = self.code >= self.range;
        if b {
            self.code -= self.range;
        }
        self.normalize();
        b
    }
}

/// A TVC range encoder for one slice.
#[derive(Debug)]
pub struct RangeEncoder {
    range: u32,
    low: u64,
    cache: u8,
    cache_size: usize,
    out: Vec<u8>,
}
impl Default for RangeEncoder {
    fn default() -> Self {
        Self {
            range: u32::MAX,
            low: 0,
            cache: 0,
            cache_size: 1,
            out: Vec::new(),
        }
    }
}
impl RangeEncoder {
    /// Create a new coder with fresh range state.
    pub fn new() -> Self {
        Self::default()
    }
    fn shift_low(&mut self) {
        if (self.low as u32) < 0xff00_0000 || self.low >> 32 != 0 {
            let carry = (self.low >> 32) as u8;
            let mut temp = self.cache;
            while self.cache_size != 0 {
                self.out.push(temp.wrapping_add(carry));
                temp = 0xff;
                self.cache_size -= 1;
            }
            self.cache = (self.low >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low & 0x00ff_ffff) << 8;
    }
    fn normalize(&mut self) {
        while self.range < 1 << 24 {
            self.range <<= 8;
            self.shift_low();
        }
    }
    /// Encode one bin and update its adaptive context.
    pub fn bit(&mut self, p: &mut Probability, bit: bool) {
        let bound = (self.range >> 15) * u32::from(p.0);
        if bit {
            self.low += u64::from(bound);
            self.range -= bound;
        } else {
            self.range = bound;
        }
        p.update(bit);
        self.normalize();
    }
    /// Encode an equiprobable bypass bin.
    pub fn bypass(&mut self, bit: bool) {
        self.range >>= 1;
        if bit {
            self.low += u64::from(self.range);
        }
        self.normalize();
    }
    /// Flush the slice and return the coded bytes.
    pub fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

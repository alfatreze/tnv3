//! Font registry and glyph rasteriser. A port of the `FontSet` / `MultiFont` classes of Tau V's reference
//! builder (`tools/tst_ref/tst_build.py`) on top of `ab_glyph` instead of Pillow/FreeType: faces are drawn at
//! 4x supersampling on one common baseline and box-downsampled to the 2-bit cell. Glyph outlines are
//! unhinted, so pixel values can differ slightly from the Python tool's.
use super::Error;
use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};
use std::collections::BTreeMap;

pub(crate) const SS: usize = 4;

/// Style slot of a face inside a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Style {
    Regular,
    Italic,
    Bold,
    BoldItalic,
}
impl Style {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "regular" => Some(Self::Regular),
            "italic" => Some(Self::Italic),
            "bold" => Some(Self::Bold),
            "bolditalic" => Some(Self::BoldItalic),
            _ => None,
        }
    }
    fn key(self) -> (bool, bool) {
        match self {
            Self::Regular => (false, false),
            Self::Italic => (false, true),
            Self::Bold => (true, false),
            Self::BoldItalic => (true, true),
        }
    }
}

/// One font file given to the builder.
#[derive(Clone)]
pub struct FontSpec {
    /// Family name (lower-cased on use); empty means `default`.
    pub family: String,
    pub style: Style,
    pub data: Vec<u8>,
    /// Name shown in warnings (usually the file name).
    pub label: String,
    /// Multiplies the font size of this face (0.3..3); `None` inherits the family's regular face.
    pub scale: Option<f64>,
}

impl FontSpec {
    /// Parses the command-line form `[NAME[:STYLE]=]FILE[@SCALE]` and reads the font file.
    pub fn from_argument(arg: &str) -> Result<Self, Error> {
        let (head, file_part) = match arg.split_once('=') {
            Some((h, f)) if !h.is_empty() && !h.contains(['/', '\\']) => (Some(h), f),
            _ => (None, arg),
        };
        let (path, scale) = match file_part.rsplit_once('@') {
            Some((p, sc)) if sc.parse::<f64>().is_ok() => (p, sc.parse::<f64>().ok()),
            _ => (file_part, None),
        };
        let (family, style) = match head {
            Some(h) => match h.split_once(':') {
                Some((n, st)) => (
                    n.to_owned(),
                    Style::parse(st).ok_or_else(|| Error::new(format!("--font {arg:?}: style must be regular, italic, bold or bolditalic")))?,
                ),
                None => (h.to_owned(), Style::Regular),
            },
            None => (String::new(), Style::Regular),
        };
        let data = std::fs::read(path).map_err(|e| Error::new(format!("font file {path}: {e}")))?;
        let label = std::path::Path::new(path).file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        Ok(FontSpec { family, style, data, label, scale })
    }
}

pub(crate) struct Face {
    pub font: FontArc,
    pub scale: f64,
    pub label: String,
}

/// Registry of faces and families. A family maps (bold, italic) to a face id.
pub(crate) struct FontSet {
    pub faces: Vec<Face>,
    fam: BTreeMap<String, BTreeMap<(bool, bool), usize>>,
    pub families: Vec<String>,
    pub default: usize,
}

impl FontSet {
    pub fn new(specs: &[FontSpec]) -> Result<Self, Error> {
        let mut s = FontSet { faces: Vec::new(), fam: BTreeMap::new(), families: Vec::new(), default: 0 };
        let mut default: Option<String> = None;
        for spec in specs {
            let name = if spec.family.trim().is_empty() { "default".to_owned() } else { spec.family.trim().to_ascii_lowercase() };
            if let Some(sc) = spec.scale {
                if !(0.3..=3.0).contains(&sc) {
                    return Err(Error::new("font scale must be 0.3..3"));
                }
            }
            let font = FontArc::try_from_vec(spec.data.clone())
                .map_err(|_| Error::new(format!("{}: not a usable TrueType/OpenType font", spec.label)))?;
            let scale = match spec.scale {
                Some(sc) => sc,
                None => s
                    .fam
                    .get(&name)
                    .and_then(|f| f.get(&(false, false)))
                    .map_or(1.0, |&id| s.faces[id].scale),
            };
            if default.is_none() {
                if spec.style != Style::Regular {
                    return Err(Error::new("the first font must be a regular face (it is the default font)"));
                }
                default = Some(name.clone());
            }
            let key = spec.style.key();
            let family = s.fam.entry(name.clone()).or_default();
            if family.contains_key(&key) {
                return Err(Error::new(format!("font {name} given twice for one style")));
            }
            family.insert(key, s.faces.len());
            s.faces.push(Face { font, scale, label: spec.label.clone() });
            if !s.families.contains(&name) {
                s.families.push(name);
            }
        }
        let Some(default) = default else { return Err(Error::new("no font given")) };
        for (n, f) in &s.fam {
            if !f.contains_key(&(false, false)) {
                return Err(Error::new(format!("font family {n} has no regular face")));
            }
        }
        s.default = s.families.iter().position(|n| *n == default).unwrap_or(0);
        Ok(s)
    }

    /// Family index for a font name used by the source; unknown names fall back to the default with a warning.
    pub fn family(&self, name: &str, warned: &mut Vec<String>, warnings: &mut Vec<String>) -> usize {
        let k = name.trim().to_ascii_lowercase();
        if let Some(i) = self.families.iter().position(|f| *f == k) {
            return i;
        }
        if !warned.contains(&k) {
            warned.push(k);
            warnings.push(format!(
                "warning: font {name:?} not given as a font, using the default font '{}'",
                self.families[self.default]
            ));
        }
        self.default
    }

    /// (face id, italic fallback). Missing bold is ignored; missing italic sets the fallback flag.
    pub fn resolve(&self, fam: usize, bold: bool, italic: bool) -> (usize, bool) {
        let f = &self.fam[&self.families[fam]];
        if let Some(&id) = f.get(&(bold, italic)) {
            return (id, false);
        }
        if italic {
            if let Some(&id) = f.get(&(false, true)) {
                return (id, false);
            }
            return (*f.get(&(bold, false)).unwrap_or(&f[&(false, false)]), true);
        }
        (f[&(false, false)], false)
    }

    pub fn has(&self, face: usize, c: char) -> bool {
        c == ' ' || self.faces[face].font.glyph_id(c) != GlyphId(0)
    }
}

/// Faces at one common size (face scale applied), one baseline, one cell.
pub(crate) struct MultiFont<'a> {
    faces: &'a [Face],
    chars: &'a BTreeMap<usize, Vec<char>>,
    pub cw: usize,
    pub ch: usize,
    /// Size in supersampled pixels (the em size of a scale-1 face).
    pub q: usize,
    baseline4: usize,
}

fn px(face: &Face, q: usize) -> PxScale {
    let size = ((q as f64 * face.scale).round() as usize).max(4) as f32;
    // `size` is the em size in pixels (what Pillow calls the font size); ab_glyph scales by the font height.
    match face.font.units_per_em() {
        Some(upem) if upem > 0.0 => PxScale::from(size * face.font.height_unscaled() / upem),
        _ => PxScale::from(size),
    }
}
fn metrics(face: &Face, q: usize) -> (usize, usize) {
    let sc = face.font.as_scaled(px(face, q));
    (sc.ascent().ceil().max(0.0) as usize, (-sc.descent()).ceil().max(0.0) as usize)
}
/// Ink box (left, top, right, bottom) of a character relative to the origin on the baseline, y down.
fn ink(face: &Face, q: usize, c: char) -> Option<(f32, f32, f32, f32)> {
    let g = face.font.glyph_id(c).with_scale_and_position(px(face, q), point(0.0, 0.0));
    let adv = advance_f(face, q, c);
    face.font.outline_glyph(g).map(|o| {
        let b = o.px_bounds();
        // Like Pillow's `getbbox(anchor="ls")`: the box spans the origin and the advance as well as the ink.
        (b.min.x.min(0.0), b.min.y, b.max.x.max(adv), b.max.y)
    })
}
/// Advance in supersampled pixels, rounded to whole pixels like FreeType's hinted advance in Pillow.
fn advance_f(face: &Face, q: usize, c: char) -> f32 {
    let sc = face.font.as_scaled(px(face, q));
    sc.h_advance(face.font.glyph_id(c)).round()
}

impl<'a> MultiFont<'a> {
    pub fn new(
        faces: &'a [Face],
        chars: &'a BTreeMap<usize, Vec<char>>,
        cw: usize,
        ch: usize,
        size: Option<f64>,
    ) -> Result<Self, Error> {
        let mut m = MultiFont { faces, chars, cw, ch, q: 0, baseline4: 0 };
        match size {
            None => m.fit()?,
            Some(s) => m.q = (s * SS as f64).round().max(1.0) as usize,
        }
        let (mut asc, mut desc) = (0, 0);
        for &f in chars.keys() {
            let (a, d) = metrics(&faces[f], m.q);
            asc = asc.max(a);
            desc = desc.max(d);
        }
        let top = (ch * SS).saturating_sub(asc + desc) / 2;
        m.baseline4 = (ch * SS - SS).min((((top + asc) + SS / 2) / SS) * SS);
        Ok(m)
    }

    pub fn advance(&self, f: usize, c: char) -> usize {
        let a = (advance_f(&self.faces[f], self.q, c) / SS as f32 + 0.5) as usize;
        a.clamp(1, self.cw)
    }

    fn shift(&self, f: usize, c: char) -> usize {
        match ink(&self.faces[f], self.q, c) {
            Some((l, ..)) if l < 0.0 => ((-l).ceil() as usize).div_ceil(SS) * SS,
            _ => 0,
        }
    }

    fn ink_ok(&self, f: usize, c: char) -> bool {
        if c == ' ' {
            return true;
        }
        let r = ink(&self.faces[f], self.q, c).map_or(0.0, |b| b.2);
        r.ceil() as usize + self.shift(f, c) <= self.cw * SS
            && (advance_f(&self.faces[f], self.q, c) / SS as f32 + 0.5) as usize <= self.cw
    }

    fn fit(&mut self) -> Result<(), Error> {
        for q in (8..=self.ch * SS).rev() {
            self.q = q;
            if self.chars.keys().any(|&f| {
                let (a, d) = metrics(&self.faces[f], q);
                a + d > self.ch * SS
            }) {
                continue;
            }
            if self.chars.iter().all(|(&f, cs)| cs.iter().all(|&c| self.ink_ok(f, c))) {
                return Ok(());
            }
        }
        Err(Error::new(format!("no font size fits the {}x{} cell", self.cw, self.ch)))
    }

    /// (advance, rows, clipped): `ch` rows of `cw` alpha values 0..3.
    pub fn raster(&self, f: usize, c: char) -> (usize, Vec<Vec<u8>>, bool) {
        let (cw, ch) = (self.cw, self.ch);
        let (w4, h4) = (cw * SS, ch * SS);
        let mut big = vec![0u8; w4 * h4];
        let mut clipped = false;
        if c != ' ' {
            let face = &self.faces[f];
            let sh = self.shift(f, c);
            if let Some((_, t, r, b)) = ink(face, self.q, c) {
                if r.ceil() as usize + sh > w4
                    || (self.baseline4 as f32 + t) < 0.0
                    || self.baseline4 as f32 + b > h4 as f32
                {
                    clipped = true;
                }
            }
            let g = face.font.glyph_id(c).with_scale_and_position(px(face, self.q), point(sh as f32, self.baseline4 as f32));
            if let Some(o) = face.font.outline_glyph(g) {
                let bounds = o.px_bounds();
                let (ox, oy) = (bounds.min.x.floor() as i64, bounds.min.y.floor() as i64);
                o.draw(|x, y, cov| {
                    let (px_, py_) = (ox + i64::from(x), oy + i64::from(y));
                    if px_ >= 0 && py_ >= 0 && (px_ as usize) < w4 && (py_ as usize) < h4 {
                        let v = (cov.clamp(0.0, 1.0) * 255.0).round() as u8;
                        let cell = &mut big[py_ as usize * w4 + px_ as usize];
                        *cell = (*cell).max(v);
                    }
                });
            }
        }
        let mut rows = Vec::with_capacity(ch);
        for y in 0..ch {
            let mut row = Vec::with_capacity(cw);
            for x in 0..cw {
                let mut sum = 0usize;
                for dy in 0..SS {
                    for dx in 0..SS {
                        sum += usize::from(big[(y * SS + dy) * w4 + x * SS + dx]);
                    }
                }
                let avg = (sum + SS * SS / 2) / (SS * SS);
                row.push(((avg * 3 + 127) / 255) as u8);
            }
            rows.push(row);
        }
        (self.advance(f, c), rows, clipped)
    }
}


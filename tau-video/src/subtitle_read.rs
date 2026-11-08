//! Subtitle area reader and reference renderer (container spec sections 6.1 to 6.3, drafts 1.3 to 1.6).
//!
//! [`parse_subtitle_area`] first runs the crate's structural validator and then returns the tracks, palettes,
//! variants, events and glyphs as plain data; every read is bounds-checked, so no input can panic it.
//! [`Variant::render`] draws the active events at an audio position exactly as section 6.3 prescribes
//! (text coverage with the palette index of the winning glyph, plus the baked outline layer); it applies none
//! of the player's display parameters (6.4: colours, box, offset, delay), so it is a reference for tests and
//! previews, not a player.
use crate::{Error, Tnv3Reader, invalid, validate_subtitle_area};
use std::io::{Read, Seek, SeekFrom};

pub const GRID_W: usize = 800;
pub const GRID_H: usize = 720;
const BREAK: u16 = 0xFFFF;
const SWITCH: u16 = 0xFFE0;

/// A glyph reference in an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphRef {
    Glyph(u16),
    /// Switch the current colour to this palette entry (0..=15).
    Colour(u8),
    LineBreak,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Audio samples (48 kHz); shown while `start <= p < end`.
    pub start: u32,
    pub end: u32,
    pub x: u16,
    pub y: u16,
    pub colour: u8,
    pub forced: bool,
    pub sdh: bool,
    pub refs: Vec<GlyphRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub advance: u8,
    /// `cell_w * cell_h` alpha values 0..=3, row-major.
    pub fill: Vec<u8>,
    /// `(cell_w + 2w) * (cell_h + 2w)` alpha values when the variant has an outline.
    pub outline: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub cell_w: u8,
    pub cell_h: u8,
    pub outline_w: u8,
    pub events: Vec<Event>,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub language: String,
    pub default: bool,
    pub forced_track: bool,
    pub palette: [[u8; 3]; 16],
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleArea {
    pub tracks: Vec<Track>,
}

/// The 800 x 720 result of drawing one variant at one position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Text coverage 0..=3 per grid pixel.
    pub text: Vec<u8>,
    /// Palette index of the glyph that set each text pixel (meaningful where `text > 0`).
    pub colour: Vec<u8>,
    /// Outline layer coverage 0..=3 (all zero when the variant has no outline).
    pub outline: Vec<u8>,
}

fn bad(at: usize) -> Error {
    invalid("E-TNV3-SUB", at as u64, "Subtitle area is inconsistent while reading.")
}
fn get(a: &[u8], at: usize, n: usize) -> Result<&[u8], Error> {
    at.checked_add(n).and_then(|e| a.get(at..e)).ok_or_else(|| bad(at))
}
fn rd16(a: &[u8], at: usize) -> Result<u16, Error> {
    let b = get(a, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
fn rd32(a: &[u8], at: usize) -> Result<u32, Error> {
    let b = get(a, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn unpack(a: &[u8], at: usize, w: usize, h: usize) -> Result<Vec<u8>, Error> {
    let rb = w.div_ceil(4);
    let raw = get(a, at, rb * h)?;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            out.push((raw[y * rb + x / 4] >> (2 * (x % 4))) & 3);
        }
    }
    Ok(out)
}

fn parse_events(a: &[u8], at: usize, len: usize) -> Result<Vec<Event>, Error> {
    let b = get(a, at, len)?;
    let count = rd32(b, 0)? as usize;
    let mut p = 4;
    let mut out = Vec::new();
    for _ in 0..count {
        let (start, end) = (rd32(b, p)?, rd32(b, p + 4)?);
        let (x, y) = (rd16(b, p + 8)?, rd16(b, p + 10)?);
        let (colour, flags, n) = (*get(b, p + 12, 1)?.first().unwrap(), *get(b, p + 13, 1)?.first().unwrap(), usize::from(rd16(b, p + 14)?));
        p += 16;
        let mut refs = Vec::with_capacity(n);
        for i in 0..n {
            let id = rd16(b, p + 2 * i)?;
            refs.push(if id == BREAK {
                GlyphRef::LineBreak
            } else if id >= SWITCH {
                GlyphRef::Colour((id - SWITCH) as u8)
            } else {
                GlyphRef::Glyph(id)
            });
        }
        p += 2 * n;
        out.push(Event { start, end, x, y, colour, forced: flags & 1 != 0, sdh: flags & 2 != 0, refs });
    }
    Ok(out)
}

fn parse_glyphs(a: &[u8], at: usize, len: usize, ow: usize) -> Result<(u8, u8, Vec<Glyph>), Error> {
    let b = get(a, at, len)?;
    let count = usize::from(rd16(b, 0)?);
    let (cw, ch) = (usize::from(b[2]), usize::from(b[3]));
    let mut p = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let advance = *get(b, p, 1)?.first().unwrap();
        p += 1;
        let fill = unpack(b, p, cw, ch)?;
        p += cw.div_ceil(4) * ch;
        let outline = if ow > 0 {
            let (w2, h2) = (cw + 2 * ow, ch + 2 * ow);
            let o = unpack(b, p, w2, h2)?;
            p += w2.div_ceil(4) * h2;
            Some(o)
        } else {
            None
        };
        out.push(Glyph { advance, fill, outline });
    }
    Ok((b[2], b[3], out))
}

/// Parses a subtitle area after validating it. `Err` for anything the validator rejects.
pub fn parse_subtitle_area(a: &[u8]) -> Result<SubtitleArea, Error> {
    validate_subtitle_area(a, 0)?;
    let nt = usize::from(a[0]);
    let mut tracks = Vec::with_capacity(nt);
    let mut pos = 4;
    for _ in 0..nt {
        let lang_raw = get(a, pos, 4)?;
        let language = lang_raw.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>();
        let (flags, nv) = (a[pos + 4], usize::from(a[pos + 5]));
        let pal_raw = get(a, pos + 8, 48)?;
        let mut palette = [[0u8; 3]; 16];
        for (k, c) in palette.iter_mut().enumerate() {
            *c = [pal_raw[3 * k], pal_raw[3 * k + 1], pal_raw[3 * k + 2]];
        }
        let mut variants = Vec::with_capacity(nv);
        for v in 0..nv {
            let e = pos + 56 + 20 * v;
            let ow = usize::from(*get(a, e + 2, 1)?.first().unwrap());
            let (eo, el, go, gl) = (rd32(a, e + 4)? as usize, rd32(a, e + 8)? as usize, rd32(a, e + 12)? as usize, rd32(a, e + 16)? as usize);
            let events = parse_events(a, eo, el)?;
            let (cw, ch, glyphs) = parse_glyphs(a, go, gl, ow)?;
            variants.push(Variant { cell_w: cw, cell_h: ch, outline_w: ow as u8, events, glyphs });
        }
        tracks.push(Track { language, default: flags & 1 != 0, forced_track: flags & 2 != 0, palette, variants });
        pos += 56 + 20 * nv;
    }
    Ok(SubtitleArea { tracks })
}

impl Variant {
    /// Events shown at audio position `p` (samples).
    pub fn active(&self, p: u32) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.start <= p && p < e.end)
    }

    /// Draws the active events at `p` as section 6.3 prescribes. A later glyph wins on equal alpha.
    pub fn render(&self, p: u32) -> Frame {
        let mut f = Frame { text: vec![0; GRID_W * GRID_H], colour: vec![0; GRID_W * GRID_H], outline: vec![0; GRID_W * GRID_H] };
        let (cw, ch, ow) = (usize::from(self.cell_w), usize::from(self.cell_h), usize::from(self.outline_w));
        for e in self.active(p) {
            let (mut px, mut py) = (usize::from(e.x), usize::from(e.y));
            let mut cur = e.colour;
            for r in &e.refs {
                match *r {
                    GlyphRef::Colour(k) => cur = k,
                    GlyphRef::LineBreak => {
                        px = usize::from(e.x);
                        py += ch;
                    }
                    GlyphRef::Glyph(id) => {
                        let Some(g) = self.glyphs.get(usize::from(id)) else { continue };
                        for y in 0..ch {
                            for x in 0..cw {
                                let a = g.fill[y * cw + x];
                                let (gx, gy) = (px + x, py + y);
                                if a > 0 && gx < GRID_W && gy < GRID_H {
                                    let i = gy * GRID_W + gx;
                                    if a >= f.text[i] {
                                        f.text[i] = a;
                                        f.colour[i] = cur;
                                    }
                                }
                            }
                        }
                        if let Some(o) = &g.outline {
                            let w2 = cw + 2 * ow;
                            for y in 0..ch + 2 * ow {
                                for x in 0..w2 {
                                    let a = o[y * w2 + x];
                                    let (gx, gy) = ((px + x).wrapping_sub(ow), (py + y).wrapping_sub(ow));
                                    if a > 0 && gx < GRID_W && gy < GRID_H {
                                        let i = gy * GRID_W + gx;
                                        f.outline[i] = f.outline[i].max(a);
                                    }
                                }
                            }
                        }
                        px += usize::from(g.advance);
                    }
                }
            }
        }
        f
    }
}

impl<R: Read + Seek> Tnv3Reader<R> {
    /// Reads and parses the subtitle area, or `None` when the file has none. Areas above
    /// [`crate::MAX_SUBTITLE_AREA_BYTES`] are refused.
    pub fn read_subtitle_area(&mut self) -> Result<Option<SubtitleArea>, Error> {
        let (flags, offset, length) = {
            let i = self.info();
            (i.flags, i.subtitle_offset, i.subtitle_length)
        };
        if flags & 4 == 0 {
            return Ok(None);
        }
        if length > crate::MAX_SUBTITLE_AREA_BYTES {
            return Err(Error::LimitExceeded { offset: u64::from(offset), message: "Subtitle area exceeds the 64 MiB limit." });
        }
        self.input.seek(SeekFrom::Start(u64::from(offset)))?;
        let mut area = vec![0u8; length as usize];
        self.input.read_exact(&mut area)?;
        parse_subtitle_area(&area).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One track, one variant 3x2 with outline 1: one glyph with a single opaque pixel, one event.
    fn tiny_area() -> Vec<u8> {
        let (cw, ch, ow) = (3usize, 2usize, 1usize);
        let mut fill = vec![0u8; cw * ch];
        fill[0] = 3;
        let mut gl = vec![1, 0, cw as u8, ch as u8, 3]; // count=1 (u16), cw, ch, advance=3
        let pack = |rows: &[u8], w: usize, h: usize| {
            let rb = w.div_ceil(4);
            let mut o = vec![0u8; rb * h];
            for y in 0..h {
                for x in 0..w {
                    o[y * rb + x / 4] |= rows[y * w + x] << (2 * (x % 4));
                }
            }
            o
        };
        gl.extend(pack(&fill, cw, ch));
        let (w2, h2) = (cw + 2 * ow, ch + 2 * ow);
        let mut plane = vec![0u8; w2 * h2];
        for (dx, dy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            plane[((1 + dy) as usize) * w2 + (1 + dx) as usize] = 3;
        }
        gl.extend(pack(&plane, w2, h2));
        let mut ev = vec![1, 0, 0, 0];
        ev.extend(100u32.to_le_bytes());
        ev.extend(200u32.to_le_bytes());
        ev.extend(10u16.to_le_bytes());
        ev.extend(20u16.to_le_bytes());
        ev.extend([2, 2]); // colour 2, flags sdh
        ev.extend(3u16.to_le_bytes());
        for id in [0u16, 0xFFE5, 0] {
            ev.extend(id.to_le_bytes());
        }
        let dir_len: usize = 4 + 56 + 20;
        let eo = dir_len.next_multiple_of(4);
        let go = (eo + ev.len()).next_multiple_of(4);
        let mut a = vec![1, 0, 0, 0];
        a.extend(b"eng\0");
        a.extend([1, 1, 0, 0]);
        for k in 0..16u8 {
            a.extend([k, k, k]);
        }
        a.extend([cw as u8, ch as u8, ow as u8, 0]);
        for v in [eo, ev.len(), go, gl.len()] {
            a.extend((v as u32).to_le_bytes());
        }
        a.resize(eo, 0);
        a.extend(&ev);
        a.resize(go, 0);
        a.extend(&gl);
        a
    }

    #[test]
    fn parses_tracks_events_glyphs_and_outline() {
        let area = parse_subtitle_area(&tiny_area()).unwrap();
        let t = &area.tracks[0];
        assert_eq!((t.language.as_str(), t.default, t.forced_track, t.palette[7]), ("eng", true, false, [7, 7, 7]));
        let v = &t.variants[0];
        assert_eq!((v.cell_w, v.cell_h, v.outline_w, v.glyphs.len()), (3, 2, 1, 1));
        let e = &v.events[0];
        assert_eq!((e.start, e.end, e.x, e.y, e.colour, e.forced, e.sdh), (100, 200, 10, 20, 2, false, true));
        assert_eq!(e.refs, [GlyphRef::Glyph(0), GlyphRef::Colour(5), GlyphRef::Glyph(0)]);
        assert_eq!(v.glyphs[0].fill[0], 3);
        assert_eq!(v.glyphs[0].outline.as_ref().unwrap().len(), 5 * 4);
    }

    #[test]
    fn render_follows_section_6_3() {
        let area = parse_subtitle_area(&tiny_area()).unwrap();
        let v = &area.tracks[0].variants[0];
        let none = v.render(99);
        assert!(none.text.iter().all(|&a| a == 0), "before start nothing is shown");
        assert!(v.render(200).text.iter().all(|&a| a == 0), "end is exclusive");
        let f = v.render(100);
        let at = |x: usize, y: usize| y * GRID_W + x;
        assert_eq!((f.text[at(10, 20)], f.colour[at(10, 20)]), (3, 2), "first glyph at the pen, event colour");
        assert_eq!((f.text[at(13, 20)], f.colour[at(13, 20)]), (3, 5), "pen advanced by 3, colour switched to 5");
        assert_eq!(f.text.iter().filter(|&&a| a > 0).count(), 2);
        assert_eq!(f.outline[at(9, 20)], 3, "outline one pixel left of the fill (offset -w)");
        assert_eq!(f.outline[at(10, 19)], 3, "outline one pixel above");
        assert_eq!(f.outline[at(11, 20)], 3);
        assert_eq!(f.outline[at(8, 20)], 0);
    }

    #[test]
    fn corrupt_areas_are_errors_not_panics() {
        let good = tiny_area();
        for cut in [0usize, 3, 10, 60, good.len() - 1] {
            assert!(parse_subtitle_area(&good[..cut]).is_err(), "cut at {cut}");
        }
        for i in 0..good.len() {
            let mut m = good.clone();
            m[i] ^= 0xFF;
            let _ = parse_subtitle_area(&m); // must not panic whatever the byte
        }
    }
}

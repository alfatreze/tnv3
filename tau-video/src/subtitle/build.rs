//! Track loading, layout, glyph table, event list and area packing (port of the reference builder).
use super::{
    Align, Error, Options, Placement, SpeakerStyle,
    font::{FontSet, MultiFont, SS},
    text::{self, AssStyle, Base, RawItem, Rgb, WHITE},
};
use std::collections::{BTreeMap, BTreeSet};

const GRID_W: usize = 800;
const GRID_H: usize = 720;
const BREAK: u16 = 0xFFFF;
const SWITCH: u16 = 0xFFE0;
const RATE: u128 = 48000;
const GREY: Rgb = (160, 160, 160);
const DEFAULT_PALETTE: [Rgb; 4] = [(255, 255, 255), (255, 230, 0), (0, 230, 255), (0, 230, 0)];

#[derive(Clone, Debug)]
pub struct VariantInfo {
    pub cell: (u32, u32),
    pub events_out: usize,
    pub glyphs: usize,
    pub glyph_bytes: usize,
    pub sub_bytes: usize,
    /// `"bar"` or `"picture"`.
    pub region: &'static str,
    /// Font size in grid pixels.
    pub size: f64,
    pub max_lines: usize,
    pub clipped: usize,
    pub sdh_events: usize,
    pub forced_events: usize,
    pub switches: usize,
    pub faces: usize,
}
#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub name: String,
    pub events_in: usize,
    pub dropped: usize,
    pub colours: usize,
    pub variants: Vec<VariantInfo>,
}
#[derive(Clone, Debug)]
pub struct BuildInfo {
    pub tracks: Vec<TrackInfo>,
    pub bytes: usize,
    pub warnings: Vec<String>,
}

/// Item after face resolution: character, face id, palette index.
#[derive(Clone, Copy)]
struct Item {
    ch: char,
    face: usize,
    idx: u8,
}
type Line = Vec<Item>;
struct Event {
    start: u64,
    end: u64,
    lines: Vec<(Line, bool)>,
}
struct Track {
    events: Vec<Event>,
    dropped: usize,
    n_in: usize,
    palette: Vec<Rgb>,
    used_colours: usize,
}

fn parse_color(s: &str) -> Option<Rgb> {
    text::html_rgb(s)
}

fn apply_speaker(
    lines: Vec<Vec<RawItem>>,
    sheet: &BTreeMap<String, SpeakerStyle>,
    fonts: &FontSet,
    warned: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Vec<Vec<RawItem>> {
    let mut out = Vec::new();
    for ln in lines {
        let chars: Vec<char> = ln.iter().map(|i| i.0).collect();
        let txt: String = chars.iter().collect();
        let off = if txt.starts_with("- ") {
            2
        } else if txt.starts_with('-') {
            1
        } else {
            0
        };
        let mut ln = ln;
        for (prefix, sty) in sheet {
            let pchars: Vec<char> = prefix.chars().collect();
            if chars.len() >= off + pchars.len() && chars[off..off + pchars.len()] == pchars[..] {
                let mut rest: Vec<RawItem> = ln[off + pchars.len()..].to_vec();
                if rest.first().is_some_and(|r| r.0 == ' ') {
                    rest.remove(0);
                }
                let fam = sty.font.as_ref().map(|f| fonts.family(f, warned, warnings));
                let rgb = sty.color.as_deref().and_then(parse_color);
                let new: Vec<RawItem> = rest
                    .into_iter()
                    .map(|(ch, (f, b, i), c)| {
                        (ch, (fam.unwrap_or(f), sty.bold.unwrap_or(b), sty.italic.unwrap_or(i)), if rgb.is_some() && c.is_none() { rgb } else { c })
                    })
                    .collect();
                ln = ln[..off].iter().cloned().chain(new).collect();
                break;
            }
        }
        if !ln.is_empty() {
            out.push(ln);
        }
    }
    out
}

fn load_track(
    name: &str,
    text: &str,
    quiet_fonts: bool,
    fonts: &FontSet,
    sheet: &BTreeMap<String, SpeakerStyle>,
    italic_rgb: Rgb,
    warnings: &mut Vec<String>,
) -> Result<Track, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lower = name.to_lowercase();
    let ass = lower.ends_with(".ass") || lower.ends_with(".ssa");
    let mut warned = Vec::new();
    let dflt = Base { fam: fonts.default, bold: false, italic: false, rgb: None };
    // raw events: (start, end, markup, style name)
    let (raw, sb, first, default_rgb): (Vec<(u64, u64, String, String)>, BTreeMap<String, Base>, Base, Rgb);
    if ass {
        let (events, styles, order) = text::parse_ass(text)?;
        let mut sbm = BTreeMap::new();
        for (n, s) in &styles {
            let AssStyle { font, rgb, bold, italic } = s.clone();
            let mut scratch = Vec::new();
            let fam = fonts.family(&font, &mut warned, if quiet_fonts { &mut scratch } else { warnings });
            sbm.insert(n.clone(), Base { fam, bold, italic, rgb: Some(rgb) });
        }
        let fst = sbm.get("Default").cloned().or_else(|| order.first().and_then(|n| sbm.get(n).cloned())).unwrap_or(dflt.clone());
        default_rgb = fst.rgb.unwrap_or(WHITE);
        raw = events.into_iter().map(|e| (e.start, e.end, e.text, e.style)).collect();
        sb = sbm;
        first = fst;
    } else {
        raw = text::parse_srt(text).into_iter().map(|(a, b, c)| (a, b, c, String::new())).collect();
        sb = BTreeMap::new();
        first = dflt;
        default_rgb = WHITE;
    }
    let n_in = raw.len();
    let mut pal: Vec<Rgb> = vec![default_rgb];
    let mut warned_pal: Vec<Rgb> = Vec::new();
    let (mut events, mut dropped) = (Vec::new(), 0usize);
    let mut badchar: BTreeSet<(usize, char)> = BTreeSet::new();
    let mut pal_index = |rgb: Rgb, warnings: &mut Vec<String>| -> u8 {
        if rgb == default_rgb {
            return 0;
        }
        if let Some(p) = pal.iter().position(|c| *c == rgb) {
            return p as u8;
        }
        if pal.len() < 16 {
            pal.push(rgb);
            return (pal.len() - 1) as u8;
        }
        let k = (0..pal.len())
            .min_by_key(|&j| {
                (0..3).map(|c| {
                    let (a, b) = ([pal[j].0, pal[j].1, pal[j].2][c] as i64, [rgb.0, rgb.1, rgb.2][c] as i64);
                    (a - b) * (a - b)
                }).sum::<i64>()
            })
            .unwrap_or(0);
        if !warned_pal.contains(&rgb) {
            warned_pal.push(rgb);
            warnings.push(format!("warning: more than 16 colours; {:02X}{:02X}{:02X} mapped to the nearest entry {k}", rgb.0, rgb.1, rgb.2));
        }
        k as u8
    };
    for (st, en, markup, sname) in raw {
        let base = sb.get(&sname).cloned().unwrap_or_else(|| first.clone());
        let mut warn_local: Vec<String> = Vec::new();
        let mut fam_for = |n: &str| fonts.family(n, &mut warned, &mut warn_local);
        let segs = text::parse_markup(&markup, ass, &base, &mut fam_for, &sb);
        if !quiet_fonts {
            warnings.append(&mut warn_local);
        }
        let mut lines = text::to_lines(&segs);
        if !sheet.is_empty() {
            lines = apply_speaker(lines, sheet, fonts, &mut warned, warnings);
        }
        if lines.is_empty() {
            dropped += 1;
            continue;
        }
        let joined = lines.iter().map(|l| l.iter().map(|i| i.0).collect::<String>()).collect::<Vec<_>>().join(" ");
        let whole = text::is_sdh_text(&joined);
        let mut out = Vec::new();
        for ln in lines {
            let sdh_line = whole || text::is_sdh_text(&ln.iter().map(|i| i.0).collect::<String>());
            let mut items = Vec::with_capacity(ln.len());
            for (ch, (fam, bold, ital), rgb) in ln {
                let (face, fb) = fonts.resolve(fam, bold, ital);
                let mut rgb = rgb.unwrap_or(default_rgb);
                if fb && rgb == default_rgb {
                    rgb = italic_rgb;
                }
                let mut ch = ch;
                if !fonts.has(face, ch) {
                    if badchar.insert((face, ch)) {
                        warnings.push(format!("warning: U+{:04X} not in font {}, replaced by ?", ch as u32, fonts.faces[face].label));
                    }
                    ch = '?';
                }
                items.push(Item { ch, face, idx: pal_index(rgb, warnings) });
            }
            out.push((items, sdh_line));
        }
        events.push(Event { start: st, end: en, lines: out });
    }
    if events.is_empty() {
        return Err(Error::new(format!("{name}: no visible events")));
    }
    let used = pal.len();
    pal.resize(16, GREY);
    Ok(Track { events, dropped, n_in, palette: pal, used_colours: used })
}

// ------------------------------------------------------------------------------------------- glyph packing

/// Grey-scale disc dilation (radius `w`) of the fill on (cw + 2w) x (ch + 2w) pixels.
fn bake_outline(rows: &[Vec<u8>], w: usize) -> Vec<Vec<u8>> {
    let (ch, cw) = (rows.len(), rows[0].len());
    let mut plane = vec![vec![0u8; cw + 2 * w]; ch + 2 * w];
    let wi = w as i64;
    let mut offs = Vec::new();
    for dx in -wi..=wi {
        for dy in -wi..=wi {
            if dx * dx + dy * dy <= wi * wi {
                offs.push((dx, dy));
            }
        }
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, &a) in row.iter().enumerate() {
            if a != 0 {
                for &(dx, dy) in &offs {
                    let cell = &mut plane[(r as i64 + wi + dy) as usize][(c as i64 + wi + dx) as usize];
                    if *cell < a {
                        *cell = a;
                    }
                }
            }
        }
    }
    plane
}

fn pack_plane(rows: &[Vec<u8>], out: &mut Vec<u8>) {
    let rowb = rows[0].len().div_ceil(4);
    for row in rows {
        let mut b = vec![0u8; rowb];
        for (x, &v) in row.iter().enumerate() {
            b[x / 4] |= v << (2 * (x % 4));
        }
        out.extend(b);
    }
}

fn pack_glyphs(cw: usize, ch: usize, glyphs: &[(usize, Vec<Vec<u8>>)], ow: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((glyphs.len() as u16).to_le_bytes());
    out.push(cw as u8);
    out.push(ch as u8);
    for (adv, rows) in glyphs {
        out.push(*adv as u8);
        pack_plane(rows, &mut out);
        if ow > 0 {
            pack_plane(&bake_outline(rows, ow), &mut out);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------- layout

/// Greedy word wrap of one line; the original space item between two words is kept.
fn wrap_line(line: &[Item], adv: &dyn Fn(&Item) -> usize, max_w: usize) -> Vec<Vec<Item>> {
    let mut words: Vec<Vec<Item>> = vec![Vec::new()];
    let mut seps: Vec<Item> = Vec::new();
    let mut gap: Option<Item> = None;
    for it in line {
        if it.ch == ' ' {
            if gap.is_none() {
                gap = Some(*it);
            }
        } else {
            if let Some(g) = gap.take() {
                if !words.last().unwrap().is_empty() {
                    seps.push(g);
                    words.push(Vec::new());
                }
            }
            words.last_mut().unwrap().push(*it);
        }
    }
    // Python filters empty words after building separators; the separators list indexes the original words.
    let keep: Vec<(usize, Vec<Item>)> = words.into_iter().enumerate().filter(|(_, w)| !w.is_empty()).collect();
    let mut pieces: Vec<(Vec<Item>, Option<Item>)> = Vec::new();
    for (wi, (orig, w)) in keep.iter().enumerate() {
        let _ = orig;
        let mut sep = if wi > 0 { seps.get(wi - 1).copied() } else { None };
        let (mut cur, mut wd) = (Vec::new(), 0usize);
        for it in w {
            let a = adv(it);
            if !cur.is_empty() && wd + a > max_w {
                pieces.push((std::mem::take(&mut cur), sep.take()));
                wd = 0;
            }
            cur.push(*it);
            wd += a;
        }
        pieces.push((cur, sep));
    }
    let (mut lines, mut cur, mut wd): (Vec<Vec<Item>>, Vec<Item>, usize) = (Vec::new(), Vec::new(), 0);
    for (piece, sep) in pieces {
        let ww: usize = piece.iter().map(adv).sum();
        let mut sw = if !cur.is_empty() && sep.is_some() { adv(sep.as_ref().unwrap()) } else { 0 };
        if !cur.is_empty() && wd + sw + ww > max_w {
            lines.push(std::mem::take(&mut cur));
            wd = 0;
            sw = 0;
        }
        if let (false, Some(s)) = (cur.is_empty(), sep) {
            cur.push(s);
        }
        cur.extend(piece);
        wd += sw + ww;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn snap(ms: u64, fps: Option<(u32, u32)>) -> u128 {
    let ms = u128::from(ms);
    match fps {
        None => ms * 48,
        Some((fnum, fden)) => {
            let (fnum, fden) = (u128::from(fnum), u128::from(fden));
            let k = (2 * ms * fnum + 1000 * fden) / (2000 * fden);
            (k * RATE * fden) / fnum
        }
    }
}

fn line_refs(items: &[Item], gid: &BTreeMap<(usize, char), u16>, mut cur: u8) -> (Vec<u16>, u8) {
    let mut ids = Vec::new();
    for it in items {
        if it.ch != ' ' && it.idx != cur {
            ids.push(SWITCH + u16::from(it.idx));
            cur = it.idx;
        }
        ids.push(gid[&(it.face, it.ch)]);
    }
    (ids, cur)
}

struct Rec {
    start: u32,
    end: u32,
    x: u16,
    y: u16,
    col: u8,
    flags: u8,
    refs: Vec<u16>,
}

#[allow(clippy::too_many_arguments)]
fn build_variant(
    track: &Track,
    fonts: &FontSet,
    cell: (u32, u32),
    o: &Options,
    forced: bool,
    outline_w: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, Vec<u8>, VariantInfo), Error> {
    let (cw, ch) = (cell.0 as usize, cell.1 as usize);
    let (vw, vh) = (o.video.0 as usize, o.video.1 as usize);
    if !(1..=255).contains(&cw) || !(1..=255).contains(&ch) {
        return Err(Error::new("cell size 1..255"));
    }
    if !(1..=GRID_W).contains(&vw) || !(1..=GRID_H).contains(&vh) {
        return Err(Error::new("video size must fit the 800x720 grid"));
    }
    let used: Vec<(usize, char)> = {
        let mut set = BTreeSet::new();
        for e in &track.events {
            for (ln, _) in &e.lines {
                for it in ln {
                    set.insert((it.face, it.ch));
                }
            }
        }
        set.into_iter().collect()
    };
    let mut chars: BTreeMap<usize, Vec<char>> = BTreeMap::new();
    for &(f, c) in &used {
        chars.entry(f).or_default().push(c);
    }
    let mf = MultiFont::new(&fonts.faces, &chars, cw, ch, o.font_size)?;
    let gid: BTreeMap<(usize, char), u16> = used.iter().enumerate().map(|(i, k)| (*k, i as u16)).collect();
    if used.len() > usize::from(SWITCH) {
        return Err(Error::new("too many glyphs"));
    }
    let adv: BTreeMap<(usize, char), usize> = used.iter().map(|&(f, c)| ((f, c), mf.advance(f, c))).collect();
    let mut glyphs = Vec::new();
    let mut clipped = 0usize;
    for &(f, c) in &used {
        let (a, rows, clip) = mf.raster(f, c);
        clipped += usize::from(clip);
        glyphs.push((a, rows));
    }
    if clipped > 0 {
        warnings.push(format!("warning: {clipped} glyphs clipped by the cell"));
    }
    let gbytes = pack_glyphs(cw, ch, &glyphs, outline_w);
    let wadv = |it: &Item| adv[&(it.face, it.ch)];
    let max_w = GRID_W - 2 * o.margin as usize;
    let mut wrapped: Vec<(u64, u64, Vec<(Vec<Item>, bool)>)> = Vec::new();
    for e in &track.events {
        let mut out = Vec::new();
        for (ln, s) in &e.lines {
            for w in wrap_line(ln, &wadv, max_w) {
                out.push((w, *s));
            }
        }
        wrapped.push((e.start, e.end, out));
    }
    let max_lines = wrapped.iter().map(|w| w.2.len()).max().unwrap_or(0);
    let max_stack = wrapped
        .iter()
        .map(|(s1, _, _)| wrapped.iter().filter(|(s2, e2, _)| s2 <= s1 && s1 < e2).map(|w| w.2.len()).sum::<usize>())
        .max()
        .unwrap_or(0);
    let pic_top = (GRID_H - vh) / 2;
    let pic_bot = pic_top + vh;
    let bar_h = GRID_H - pic_bot;
    let bottom = o.bottom as usize;
    let need = max_stack * ch + bottom + 8;
    let (region, base_bottom): (&'static str, i64) = if o.placement == Placement::Bar || (o.placement == Placement::Auto && need <= bar_h) {
        ("bar", (GRID_H - bottom) as i64)
    } else {
        ("picture", pic_bot as i64 - bottom as i64)
    };
    if region == "picture" && base_bottom - ((max_stack * ch) as i64) < 0 {
        return Err(Error::new("text block does not fit above the picture bottom"));
    }
    let mut order: Vec<usize> = (0..wrapped.len()).collect();
    order.sort_by_key(|&i| (wrapped[i].0, i));
    let mut active: Vec<(u128, i64, i64)> = Vec::new();
    let mut recs: Vec<Rec> = Vec::new();
    let fbit = u8::from(forced);
    for i in order {
        let (st_ms, en_ms, lines) = &wrapped[i];
        let s0 = snap(*st_ms, o.fps);
        let mut s1 = snap(*en_ms, o.fps);
        if s1 <= s0 {
            s1 = s0 + match o.fps {
                None => 1,
                Some((n, d)) => (RATE * u128::from(d)).div_ceil(u128::from(n)),
            };
        }
        if s1 > 0xFFFF_FFFF {
            return Err(Error::new("time exceeds 32 bits"));
        }
        let h = (lines.len() * ch) as i64;
        if h > GRID_H as i64 {
            return Err(Error::new(format!(
                "The event at {st_ms} ms has {} lines of {ch} rows, which do not fit the {GRID_H}-row grid; use a smaller size or shorter text.",
                lines.len()
            )));
        }
        active.retain(|a| a.0 > s0);
        let mut bot = base_bottom - if o.center_single && lines.len() == 1 && active.is_empty() { (ch / 2) as i64 } else { 0 };
        let mut moved = true;
        while moved {
            moved = false;
            for &(_, yt, yb) in &active {
                if yt < bot && bot - h < yb {
                    bot = yt;
                    moved = true;
                }
            }
        }
        let mut ytop = bot - h;
        if ytop < 0 {
            warnings.push(format!("warning: event at {st_ms} ms does not fit above the stack; clamped"));
            ytop = 0;
        }
        active.push((s1, ytop, ytop + h));
        let widths: Vec<usize> = lines.iter().map(|(ln, _)| ln.iter().map(&wadv).sum()).collect();
        let (s0, s1) = (s0 as u32, s1 as u32);
        if o.align == Align::Block && lines.len() > 1 && lines.iter().all(|(_, s)| *s == lines[0].1) {
            let (mut ids, mut cur) = (Vec::new(), lines[0].0[0].idx);
            for (j, (ln, _)) in lines.iter().enumerate() {
                if j > 0 {
                    ids.push(BREAK);
                }
                let (r, c2) = line_refs(ln, &gid, cur);
                ids.extend(r);
                cur = c2;
            }
            let fl = fbit | if o.sdh && lines[0].1 { 2 } else { 0 };
            let wmax = widths.iter().copied().max().unwrap_or(0);
            recs.push(Rec { start: s0, end: s1, x: ((GRID_W - wmax) / 2) as u16, y: ytop as u16, col: lines[0].0[0].idx, flags: fl, refs: ids });
            continue;
        }
        for (j, (ln, s)) in lines.iter().enumerate() {
            let fl = fbit | if o.sdh && *s { 2 } else { 0 };
            let (ids, _) = line_refs(ln, &gid, ln[0].idx);
            recs.push(Rec {
                start: s0,
                end: s1,
                x: ((GRID_W - widths[j]) / 2) as u16,
                y: (ytop + (j * ch) as i64) as u16,
                col: ln[0].idx,
                flags: fl,
                refs: ids,
            });
        }
    }
    recs.sort_by_key(|r| r.start);
    let mut sub = Vec::new();
    sub.extend((recs.len() as u32).to_le_bytes());
    for r in &recs {
        if usize::from(r.x) >= GRID_W || usize::from(r.y) >= GRID_H {
            return Err(Error::new(format!("position out of grid: {},{}", r.x, r.y)));
        }
        sub.extend(r.start.to_le_bytes());
        sub.extend(r.end.to_le_bytes());
        sub.extend(r.x.to_le_bytes());
        sub.extend(r.y.to_le_bytes());
        sub.push(r.col);
        sub.push(r.flags);
        sub.extend((r.refs.len() as u16).to_le_bytes());
        for id in &r.refs {
            sub.extend(id.to_le_bytes());
        }
    }
    let info = VariantInfo {
        cell,
        events_out: recs.len(),
        glyphs: glyphs.len(),
        glyph_bytes: gbytes.len(),
        sub_bytes: sub.len(),
        region,
        size: mf.q as f64 / SS as f64,
        max_lines,
        clipped,
        sdh_events: recs.iter().filter(|r| r.flags & 2 != 0).count(),
        forced_events: recs.iter().filter(|r| r.flags & 1 != 0).count(),
        switches: recs.iter().flat_map(|r| &r.refs).filter(|g| (SWITCH..=SWITCH + 15).contains(g)).count(),
        faces: chars.len(),
    };
    Ok((sub, gbytes, info))
}

// -------------------------------------------------------------------------------------------- area packing

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

struct PackTrack {
    lang: String,
    default: bool,
    forced: bool,
    palette: Vec<Rgb>,
    variants: Vec<(Vec<u8>, Vec<u8>)>,
    outline_w: Vec<usize>,
}

fn pack_area(tracks: &[PackTrack]) -> Result<Vec<u8>, Error> {
    if !(1..=15).contains(&tracks.len()) {
        return Err(Error::new("1..15 tracks"));
    }
    let dir_len = 4 + tracks.iter().map(|t| 56 + 20 * t.variants.len()).sum::<usize>();
    let mut pos = align4(dir_len);
    let mut body = vec![0u8; pos - dir_len];
    let mut entries = vec![tracks.len() as u8, 0, 0, 0];
    let mut last_end = 0usize;
    for t in tracks {
        let lang = t.lang.as_bytes();
        if !(2..=4).contains(&lang.len()) || !lang.iter().all(|c| c.is_ascii_lowercase()) {
            return Err(Error::new(format!("language code: {:?}", t.lang)));
        }
        let nv = t.variants.len();
        if !(1..=8).contains(&nv) {
            return Err(Error::new("1..8 variants"));
        }
        let flags = u8::from(t.default) | if t.forced { 2 } else { 0 };
        let mut pal = t.palette.clone();
        if pal.len() != 16 {
            pal.resize(16, GREY);
        }
        entries.extend(lang);
        entries.extend(std::iter::repeat_n(0u8, 4 - lang.len()));
        entries.extend([flags, nv as u8, 0, 0]);
        for c in &pal {
            entries.extend([c.0, c.1, c.2]);
        }
        for ((ev, gl), &ow) in t.variants.iter().zip(&t.outline_w) {
            let eo = pos;
            body.extend(ev);
            pos += ev.len();
            let pad = align4(pos) - pos;
            body.extend(std::iter::repeat_n(0u8, pad));
            pos += pad;
            let go = pos;
            body.extend(gl);
            pos += gl.len();
            entries.extend([gl[2], gl[3], ow as u8, 0]);
            entries.extend((eo as u32).to_le_bytes());
            entries.extend((ev.len() as u32).to_le_bytes());
            entries.extend((go as u32).to_le_bytes());
            entries.extend((gl.len() as u32).to_le_bytes());
            last_end = go + gl.len();
            let pad = align4(pos) - pos;
            body.extend(std::iter::repeat_n(0u8, pad));
            pos += pad;
        }
    }
    debug_assert_eq!(entries.len(), dir_len);
    entries.extend(body);
    entries.truncate(last_end);
    Ok(entries)
}

pub(crate) fn build_area(o: &Options) -> Result<(Vec<u8>, BuildInfo), Error> {
    let fonts = FontSet::new(&o.fonts)?;
    if o.tracks.is_empty() {
        return Err(Error::new("no tracks"));
    }
    if o.cells.is_empty() {
        return Err(Error::new("no sizes"));
    }
    for w in o.cells.windows(2) {
        if w[1].1 <= w[0].1 {
            return Err(Error::new("sizes must have strictly increasing height"));
        }
    }
    if o.font_size.is_some() && o.cells.len() > 1 {
        return Err(Error::new("a fixed font size needs a single cell size"));
    }
    let ows: Vec<usize> = if o.outline_widths.len() == 1 {
        vec![usize::from(o.outline_widths[0]); o.cells.len()]
    } else {
        o.outline_widths.iter().map(|&w| usize::from(w)).collect()
    };
    if ows.len() != o.cells.len() || ows.iter().any(|&w| w > 4) {
        return Err(Error::new("outline widths: one value 0..4 per size"));
    }
    let mut warnings = Vec::new();
    let (mut packed, mut infos) = (Vec::new(), Vec::new());
    for t in &o.tracks {
        let trk = load_track(&t.name, &t.text, t.quiet_font_fallback, &fonts, &o.styles, o.italic_rgb, &mut warnings)?;
        let (mut vs, mut vinfo) = (Vec::new(), Vec::new());
        for (&cell, &ow) in o.cells.iter().zip(&ows) {
            let (ev, gl, inf) = build_variant(&trk, &fonts, cell, o, t.forced, ow, &mut warnings)?;
            vs.push((ev, gl));
            vinfo.push(inf);
        }
        let mut palette = trk.palette.clone();
        let _ = DEFAULT_PALETTE;
        palette.truncate(16);
        packed.push(PackTrack {
            lang: t.language.clone(),
            default: t.default,
            forced: t.forced,
            palette,
            variants: vs,
            outline_w: ows.clone(),
        });
        infos.push(TrackInfo { name: t.name.clone(), events_in: trk.n_in, dropped: trk.dropped, colours: trk.used_colours, variants: vinfo });
    }
    let area = pack_area(&packed)?;
    crate::validate_subtitle_area(&area, 0).map_err(|e| Error::new(format!("area failed the container check: {e}")))?;
    let bytes = area.len();
    if bytes > super::POCKET_AREA_LIMIT {
        warnings.push(format!(
            "warning: the subtitle area is {bytes} bytes; the Tau V Pocket player accepts about 1 MB. Use fewer sizes, fewer text styles (each bold or italic face adds glyphs) or shorter text."
        ));
    }
    Ok((area, BuildInfo { tracks: infos, bytes, warnings }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(c: char) -> Item {
        Item { ch: c, face: 0, idx: 0 }
    }
    fn line(s: &str) -> Vec<Item> {
        s.chars().map(item).collect()
    }
    fn text(l: &[Item]) -> String {
        l.iter().map(|i| i.ch).collect()
    }
    #[test]
    fn wrap_keeps_words_and_splits_overlong_ones() {
        let adv = |_: &Item| 10usize;
        let w: Vec<String> = wrap_line(&line("aa bb cc dd"), &adv, 50).iter().map(|l| text(l)).collect();
        assert_eq!(w, ["aa bb", "cc dd"]);
        let w: Vec<String> = wrap_line(&line("abcdefgh"), &adv, 30).iter().map(|l| text(l)).collect();
        assert_eq!(w, ["abc", "def", "gh"]);
        assert!(wrap_line(&line("   "), &adv, 50).is_empty());
    }
    #[test]
    fn snapping_matches_the_reference_formulas() {
        assert_eq!(snap(1000, None), 48_000);
        // 24 fps: 1 s -> picture 24 -> 24 * 48000 / 24
        assert_eq!(snap(1000, Some((24, 1))), 48_000);
        assert_eq!(snap(1020, Some((24, 1))), 48_000, "1.02 s snaps to picture 24");
        assert_eq!(snap(1030, Some((24, 1))), 50_000, "1.03 s snaps to picture 25");
    }
    #[test]
    fn outline_is_a_disc_dilation_and_packs_padded_rows() {
        let rows = vec![vec![0, 3, 0]];
        let plane = bake_outline(&rows, 1);
        assert_eq!(plane.len(), 3);
        assert_eq!(plane[1], vec![0u8, 3, 3, 3, 0]);
        assert_eq!(plane[0], vec![0u8, 0, 3, 0, 0]);
        assert_eq!(plane[2], vec![0u8, 0, 3, 0, 0]);
        let mut out = Vec::new();
        pack_plane(&[vec![1, 2, 3, 0, 1]], &mut out);
        assert_eq!(out, [0b00111001, 0b00000001]);
    }
}

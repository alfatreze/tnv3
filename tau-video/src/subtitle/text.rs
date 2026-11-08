//! SRT / ASS parsing and markup handling: a port of the input side of Tau V's reference builder.
//! Differences from the Python tool: HTML entities are limited to the numeric forms and the common named
//! ones listed in [`unescape`]; Unicode categories for dropping characters are approximated (control,
//! format, private-use and line/paragraph separators).
use super::Error;
use regex::Regex;
use std::{collections::BTreeMap, sync::OnceLock};

pub(crate) type Rgb = (u8, u8, u8);
pub(crate) const WHITE: Rgb = (255, 255, 255);

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("built-in pattern"))
}

fn ms(h: &str, m: &str, s: &str, frac: &str) -> u64 {
    let mut f = frac.to_owned();
    while f.len() < 3 {
        f.push('0');
    }
    let n = |x: &str| x.parse::<u64>().unwrap_or(0);
    ((n(h) * 60 + n(m)) * 60 + n(s)) * 1000 + n(&f[..3])
}

/// (start_ms, end_ms, markup) in file order.
pub(crate) fn parse_srt(text: &str) -> Vec<(u64, u64, String)> {
    static T: OnceLock<Regex> = OnceLock::new();
    let time = re(&T, r"(\d+):(\d+):(\d+)[,.](\d+)\s*-->\s*(\d+):(\d+):(\d+)[,.](\d+)");
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(m) = time.captures(lines[i]) else {
            i += 1;
            continue;
        };
        let g = |k: usize| m.get(k).map_or("", |x| x.as_str());
        let (st, en) = (ms(g(1), g(2), g(3), g(4)), ms(g(5), g(6), g(7), g(8)));
        i += 1;
        let mut body = Vec::new();
        while i < lines.len() && !lines[i].trim().is_empty() {
            body.push(lines[i]);
            i += 1;
        }
        out.push((st, en, body.join("\n")));
    }
    out
}

/// ASS colour `&H[AA]BBGGRR[&]` -> (r, g, b).
pub(crate) fn ass_rgb(s: &str) -> Option<Rgb> {
    static T: OnceLock<Regex> = OnceLock::new();
    let m = re(&T, r"&H([0-9A-Fa-f]+)").captures(s.trim())?;
    let v = u64::from_str_radix(&m[1], 16).ok()?;
    Some(((v & 255) as u8, ((v >> 8) & 255) as u8, ((v >> 16) & 255) as u8))
}

#[derive(Clone, Debug)]
pub(crate) struct AssStyle {
    pub font: String,
    pub rgb: Rgb,
    pub bold: bool,
    pub italic: bool,
}

pub(crate) struct AssEvent {
    pub start: u64,
    pub end: u64,
    pub text: String,
    pub style: String,
}

pub(crate) fn parse_ass(text: &str) -> Result<(Vec<AssEvent>, BTreeMap<String, AssStyle>, Vec<String>), Error> {
    static T: OnceLock<Regex> = OnceLock::new();
    let time = re(&T, r"^\s*(\d+):(\d+):(\d+)[.](\d+)\s*$");
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let (mut section, mut fields, mut sfields) = (String::new(), None::<Vec<String>>, None::<Vec<String>>);
    let (mut out, mut styles, mut order) = (Vec::new(), BTreeMap::new(), Vec::new());
    for line in text.split('\n') {
        let s = line.trim();
        if s.starts_with('[') {
            section = s.to_lowercase();
            continue;
        }
        let low = s.to_lowercase();
        let list = |x: &str| x.split(',').map(|f| f.trim().to_lowercase()).collect::<Vec<_>>();
        if section.starts_with("[v4") && section.contains("styles") {
            if low.starts_with("format:") {
                sfields = Some(list(&s[7..]));
            } else if low.starts_with("style:") {
                if let Some(sf) = &sfields {
                    let parts: Vec<&str> = s[6..].trim_start().splitn(sf.len(), ',').collect();
                    let get = |k: &str| sf.iter().position(|f| f == k).and_then(|i| parts.get(i)).copied().unwrap_or("");
                    let name = get("name").trim().to_owned();
                    order.push(name.clone());
                    styles.insert(
                        name,
                        AssStyle {
                            font: get("fontname").trim().to_owned(),
                            rgb: ass_rgb(get("primarycolour")).unwrap_or(WHITE),
                            bold: !matches!(get("bold").trim(), "0" | ""),
                            italic: !matches!(get("italic").trim(), "0" | ""),
                        },
                    );
                }
            }
            continue;
        }
        if section != "[events]" {
            continue;
        }
        if low.starts_with("format:") {
            fields = Some(list(&s[7..]));
        } else if low.starts_with("dialogue:") {
            let Some(f) = &fields else { return Err(Error::new("ASS Dialogue before Format")) };
            let parts: Vec<&str> = s[9..].trim_start().splitn(f.len(), ',').collect();
            if parts.len() != f.len() {
                return Err(Error::new(format!("ASS Dialogue has too few fields: {s:?}")));
            }
            let get = |k: &str| f.iter().position(|x| x == k).and_then(|i| parts.get(i)).copied();
            let mut t = [0u64; 2];
            for (n, key) in ["start", "end"].iter().enumerate() {
                let v = get(key).ok_or_else(|| Error::new(format!("ASS field {key} missing")))?;
                let m = time.captures(v).ok_or_else(|| Error::new(format!("ASS time {v:?}")))?;
                t[n] = ms(&m[1], &m[2], &m[3], &m[4]);
            }
            out.push(AssEvent {
                start: t[0],
                end: t[1],
                text: get("text").unwrap_or("").to_owned(),
                style: get("style").map_or("Default", str::trim).to_owned(),
            });
        }
    }
    // First style in file order is the fallback, so keep the order for the caller.
    let mut ordered = Vec::new();
    if let Some(first) = order.first() {
        ordered.push(first.clone());
    }
    Ok((out, styles, ordered))
}

pub(crate) fn html_rgb(s: &str) -> Option<Rgb> {
    let s = s.trim().to_lowercase();
    let named = match s.as_str() {
        "white" => Some(WHITE),
        "black" => Some((0, 0, 0)),
        "red" => Some((255, 0, 0)),
        "green" => Some((0, 128, 0)),
        "lime" => Some((0, 255, 0)),
        "blue" => Some((0, 0, 255)),
        "yellow" => Some((255, 255, 0)),
        "cyan" | "aqua" => Some((0, 255, 255)),
        "magenta" => Some((255, 0, 255)),
        "orange" => Some((255, 165, 0)),
        "gray" | "grey" => Some((128, 128, 128)),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let hex = s.strip_prefix('#').unwrap_or(&s);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return Some(((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    None
}

/// HTML entity decoding: numeric forms and the common named ones.
pub(crate) fn unescape(s: &str) -> String {
    static T: OnceLock<Regex> = OnceLock::new();
    re(&T, r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[A-Za-z][A-Za-z0-9]*);?").replace_all(s, |c: &regex::Captures| {
        let whole = c[0].to_owned();
        let body = &c[1];
        let named = match body {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "hellip" => Some('…'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            "iexcl" => Some('¡'),
            "iquest" => Some('¿'),
            "middot" => Some('·'),
            "bull" => Some('•'),
            "deg" => Some('°'),
            "times" => Some('×'),
            "euro" => Some('€'),
            "pound" => Some('£'),
            "yen" => Some('¥'),
            "cent" => Some('¢'),
            "sect" => Some('§'),
            "para" => Some('¶'),
            "trade" => Some('™'),
            _ => None,
        };
        if let Some(ch) = named {
            return ch.to_string();
        }
        let num = if let Some(h) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok()
        } else if let Some(d) = body.strip_prefix('#') {
            d.parse::<u32>().ok()
        } else {
            None
        };
        match num.and_then(char::from_u32) {
            Some(ch) if ch != '\0' => ch.to_string(),
            _ => whole,
        }
    })
    .into_owned()
}

/// Current text style while walking markup. `fam` is a family index; `rgb` None = default colour.
#[derive(Clone, Debug)]
pub(crate) struct Base {
    pub fam: usize,
    pub bold: bool,
    pub italic: bool,
    pub rgb: Option<Rgb>,
}

pub(crate) struct Seg {
    pub text: String,
    pub fam: usize,
    pub bold: bool,
    pub italic: bool,
    pub rgb: Option<Rgb>,
}

/// Handles `<i> <b> <font color face>` (SRT) and `{\i \b \fn \c \1c \r \p}` (ASS override blocks, also
/// accepted in SRT); every other tag is dropped, ASS drawings (`\p1`) are dropped, positioning is ignored.
pub(crate) fn parse_markup(
    s: &str,
    ass: bool,
    base: &Base,
    fam_for: &mut dyn FnMut(&str) -> usize,
    styles: &BTreeMap<String, Base>,
) -> Vec<Seg> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static OV: OnceLock<Regex> = OnceLock::new();
    static ATTR: OnceLock<Regex> = OnceLock::new();
    static NAME: OnceLock<Regex> = OnceLock::new();
    let tag_re = re(&TAG, r"</?[A-Za-z][^<>]*>|\{[^{}]*\}");
    let ov_re = re(&OV, r"\\([^\\]*)");
    let attr_re = re(&ATTR, r#"(?i)(color|face)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#);
    let name_re = re(&NAME, r"</?([A-Za-z]+)");
    let (p_re, c_re, i_re, b_re) = {
        static P: OnceLock<Regex> = OnceLock::new();
        static C: OnceLock<Regex> = OnceLock::new();
        static I: OnceLock<Regex> = OnceLock::new();
        static B: OnceLock<Regex> = OnceLock::new();
        (
            re(&P, r"^p\d+$"),
            re(&C, r"^1?c(&H[0-9A-Fa-f]+&?)?$"),
            re(&I, r"^i[01]?$"),
            re(&B, r"^b\d*$"),
        )
    };
    let mut segs = Vec::new();
    let mut st = base.clone();
    let mut fstack: Vec<(usize, Option<Rgb>)> = Vec::new();
    let mut drawing = false;
    let mut pos = 0;
    let mut add = |t: &str, st: &Base, drawing: bool| {
        if drawing || t.is_empty() {
            return;
        }
        let t = if ass { t.replace("\\N", "\n").replace("\\n", " ").replace("\\h", " ") } else { unescape(t) };
        segs.push(Seg { text: t, fam: st.fam, bold: st.bold, italic: st.italic, rgb: st.rgb });
    };
    for m in tag_re.find_iter(s) {
        add(&s[pos..m.start()], &st, drawing);
        pos = m.end();
        let tag = m.as_str();
        if tag.starts_with('{') {
            for c in ov_re.captures_iter(&tag[1..tag.len() - 1]) {
                let t = &c[1];
                if p_re.is_match(t) {
                    drawing = t[1..].parse::<u32>().unwrap_or(0) != 0;
                } else if let Some(name) = t.strip_prefix("fn") {
                    st.fam = if name.is_empty() { base.fam } else { fam_for(name) };
                } else if c_re.is_match(t) {
                    st.rgb = if t.contains('&') { ass_rgb(t) } else { base.rgb };
                } else if i_re.is_match(t) {
                    st.italic = if t.len() > 1 { t == "i1" } else { base.italic };
                } else if b_re.is_match(t) {
                    st.bold = if t.len() > 1 { t[1..].parse::<u32>().unwrap_or(0) >= 1 } else { base.bold };
                } else if t == "r" {
                    st = base.clone();
                } else if let Some(n) = t.strip_prefix('r') {
                    if let Some(b) = styles.get(n) {
                        st = b.clone();
                    }
                }
            }
        } else {
            let name = name_re.captures(tag).map(|c| c[1].to_lowercase()).unwrap_or_default();
            let closing = tag.starts_with("</");
            match name.as_str() {
                "i" => st.italic = !closing,
                "b" => st.bold = !closing,
                "font" => {
                    if closing {
                        if let Some((f, c)) = fstack.pop() {
                            st.fam = f;
                            st.rgb = c;
                        }
                    } else {
                        fstack.push((st.fam, st.rgb));
                        for am in attr_re.captures_iter(tag) {
                            let v = am.get(2).or(am.get(3)).or(am.get(4)).map_or("", |x| x.as_str());
                            if am[1].eq_ignore_ascii_case("color") {
                                if let Some(c) = html_rgb(v) {
                                    st.rgb = Some(c);
                                }
                            } else {
                                st.fam = fam_for(v);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    add(&s[pos..], &st, drawing);
    segs
}

/// One source character with its style, before faces are resolved.
pub(crate) type RawItem = (char, (usize, bool, bool), Option<Rgb>);

fn is_dropped(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E
            | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
            | 0xE000..=0xF8FF | 0xF0000..=0x10FFFF | 0x2028 | 0x2029)
}
fn is_space_separator(c: char) -> bool {
    matches!(c as u32, 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000)
}

/// Lines of items; whitespace collapsed, lines trimmed, empty lines dropped.
pub(crate) fn to_lines(segs: &[Seg]) -> Vec<Vec<RawItem>> {
    use unicode_normalization::UnicodeNormalization;
    let mut lines: Vec<Vec<RawItem>> = vec![Vec::new()];
    for seg in segs {
        for ch in seg.text.nfc() {
            if ch == '\n' {
                lines.push(Vec::new());
                continue;
            }
            let ch = if matches!(ch, '\t' | '\r' | ' ') || is_space_separator(ch) {
                ' '
            } else if is_dropped(ch) {
                continue;
            } else {
                ch
            };
            lines.last_mut().unwrap().push((ch, (seg.fam, seg.bold, seg.italic), seg.rgb));
        }
    }
    let mut res = Vec::new();
    for ln in lines {
        let mut out: Vec<RawItem> = Vec::new();
        for it in ln {
            if it.0 == ' ' && out.last().is_none_or(|l| l.0 == ' ') {
                continue;
            }
            out.push(it);
        }
        while out.last().is_some_and(|l| l.0 == ' ') {
            out.pop();
        }
        if !out.is_empty() {
            res.push(out);
        }
    }
    res
}

/// A line that is entirely a bracketed sound description (`[...]` or `(...)`) or only music notes.
pub(crate) fn is_sdh_text(s: &str) -> bool {
    static T: OnceLock<Regex> = OnceLock::new();
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.chars().all(|c| matches!(c, '♪' | '♫' | ' ')) {
        return true;
    }
    let group = r"(?:\[[^\[\]]*\]|\([^()]*\))";
    re(&T, &format!(r"^(?:-\s*)?{group}(?:\s*(?:-\s*)?{group})*$")).is_match(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn srt_events_and_entities() {
        let ev = parse_srt("1\r\n00:00:01,500 --> 00:00:02,000\r\nA &amp; B\r\nline two\r\n\r\n2\r\n00:01:00.5 --> 00:01:01.25\r\nx\r\n");
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].0, ev[0].1), (1500, 2000));
        assert_eq!((ev[1].0, ev[1].1), (60_500, 61_250));
        assert_eq!(unescape("A &amp; B &#65;&#x42; &bogus; &lt;"), "A & B AB &bogus; <");
    }
    #[test]
    fn ass_colours_and_events() {
        assert_eq!(ass_rgb("&H00FF8040"), Some((0x40, 0x80, 0xFF)));
        let (ev, styles, _) = parse_ass("[V4+ Styles]\nFormat: Name, Fontname, PrimaryColour, Bold, Italic\nStyle: Default,Roboto,&H00FFFFFF,0,-1\n[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:01.00,0:00:02.50,Default,Hello\\Nthere, ok\n").unwrap();
        assert_eq!((ev[0].start, ev[0].end, ev[0].text.as_str()), (1000, 2500, "Hello\\Nthere, ok"));
        assert!(styles["Default"].italic && !styles["Default"].bold);
    }
    #[test]
    fn markup_italic_colour_and_lines() {
        let base = Base { fam: 0, bold: false, italic: false, rgb: None };
        let mut f = |_: &str| 0usize;
        let segs = parse_markup("a <i>b</i> <font color=\"#FF0000\">c</font>\nd", false, &base, &mut f, &BTreeMap::new());
        let lines = to_lines(&segs);
        assert_eq!(lines.len(), 2);
        let first: String = lines[0].iter().map(|i| i.0).collect();
        assert_eq!(first, "a b c");
        assert!(lines[0][2].1.2, "b is italic");
        assert_eq!(lines[0][4].2, Some((255, 0, 0)));
        let drawn = parse_markup("{\\p1}m 0 0{\\p0}x", true, &base, &mut f, &BTreeMap::new());
        assert_eq!(drawn.iter().map(|s| s.text.as_str()).collect::<String>(), "x");
    }
    #[test]
    fn sdh_detection() {
        assert!(is_sdh_text("[door slams]"));
        assert!(is_sdh_text("- (laughs) [sighs]"));
        assert!(is_sdh_text("♪ ♪"));
        assert!(!is_sdh_text("[a] and text"));
        assert!(!is_sdh_text(""));
    }
}

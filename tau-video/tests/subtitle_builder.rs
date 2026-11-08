//! Native subtitle builder tests. They need a TrueType font: set `TAU_TEST_FONT` (for example Tau V's
//! `tools/tst_ref/fonts/Roboto-Regular.ttf`); without it each test returns early (see `make rust-conformance`).
#![cfg(feature = "subtitle-builder")]
use tau_video::subtitle::{self, Align, FontSpec, Options, Style, TrackInput};

fn font() -> Option<FontSpec> {
    let path = std::env::var_os("TAU_TEST_FONT")?;
    Some(FontSpec { family: String::new(), style: Style::Regular, data: std::fs::read(&path).ok()?, label: "test".into(), scale: None })
}
fn track(text: &str) -> TrackInput {
    TrackInput { name: "t.srt".into(), text: text.into(), language: "eng".into(), default: true, forced: false, quiet_font_fallback: false }
}
const SRT: &str = "1\n00:00:01,000 --> 00:00:02,500\nHello <i>world</i>\n\n2\n00:00:02,000 --> 00:00:04,000\n[door slams]\n\n3\n00:00:05,000 --> 00:00:06,000\nA much longer line that has to be wrapped because it is wider than the seven hundred and twenty pixel subtitle width of the grid\n";

fn opts(f: FontSpec, srt: &str) -> Options {
    let mut o = Options::new(vec![track(srt)], vec![f], (400, 224));
    o.fps = Some((24, 1));
    o.cells = vec![(16, 24), (20, 28)];
    o
}

#[test]
fn builds_a_valid_deterministic_area_with_flags_and_wrapping() {
    let Some(f) = font() else { return };
    let (a, info) = subtitle::build_area(&opts(f.clone(), SRT)).unwrap();
    let (b, _) = subtitle::build_area(&opts(f, SRT)).unwrap();
    assert_eq!(a, b, "same input, same bytes");
    assert_eq!(a[0], 1, "one track");
    assert_eq!(&a[4..8], b"eng\0");
    assert_eq!(a[8] & 1, 1, "default flag");
    let t = &info.tracks[0];
    assert_eq!((t.events_in, t.dropped, t.variants.len()), (3, 0, 2));
    for v in &t.variants {
        assert!(v.sdh_events >= 1, "the bracketed line is flagged SDH");
        assert!(v.events_out >= 4, "the long line wraps into several events: {}", v.events_out);
        assert_eq!(v.switches, 1, "one switch to the italic fallback colour (a line ends in it)");
    }
    // the area passes this crate's own validator again, byte for byte
    assert!(info.warnings.is_empty(), "{:?}", info.warnings);
}

#[test]
fn outline_widths_grow_the_glyph_table_and_block_alignment_merges_lines() {
    let Some(f) = font() else { return };
    let mut plain = opts(f.clone(), SRT);
    plain.outline_widths = vec![0];
    let (a, _) = subtitle::build_area(&plain).unwrap();
    let (b, _) = subtitle::build_area(&opts(f.clone(), SRT)).unwrap();
    assert!(b.len() > a.len(), "baked outline planes add bytes");
    let mut per_size = opts(f.clone(), SRT);
    per_size.outline_widths = vec![1, 3];
    subtitle::build_area(&per_size).unwrap();
    per_size.outline_widths = vec![1, 2, 3];
    assert!(subtitle::build_area(&per_size).is_err(), "one width per size");
    per_size.outline_widths = vec![5];
    assert!(subtitle::build_area(&per_size).is_err(), "radius above 4");
    let mut block = opts(f.clone(), SRT);
    block.align = Align::Block;
    let (_, bi) = subtitle::build_area(&block).unwrap();
    let (_, ci) = subtitle::build_area(&opts(f, SRT)).unwrap();
    assert!(bi.tracks[0].variants[0].events_out < ci.tracks[0].variants[0].events_out);
}

#[test]
fn rejects_bad_input_and_reports_the_placement_clamp() {
    let Some(f) = font() else { return };
    let mut o = opts(f.clone(), "no events here");
    assert!(subtitle::build_area(&o).unwrap_err().0.contains("no visible events"));
    o = opts(f.clone(), SRT);
    o.cells = vec![(20, 28), (16, 24)];
    assert!(subtitle::build_area(&o).unwrap_err().0.contains("increasing height"));
    o = opts(f.clone(), SRT);
    o.tracks[0].language = "ENG".into();
    assert!(subtitle::build_area(&o).unwrap_err().0.contains("language"));
    o = opts(f.clone(), SRT);
    o.fonts.clear();
    assert!(subtitle::build_area(&o).is_err());
    // many simultaneous multi-line events cannot all stack above row 0: warned, not hidden
    let mut crowd = String::new();
    for i in 0..40 {
        crowd.push_str(&format!("{}\n00:00:01,{:03} --> 00:00:09,000\nline a{i}\nline b{i}\nline c{i}\n\n", i + 1, i));
    }
    let mut c = opts(f, &crowd);
    c.placement = subtitle::Placement::Bar;
    c.cells = vec![(20, 28)];
    let (_, info) = subtitle::build_area(&c).unwrap();
    assert!(info.warnings.iter().any(|w| w.contains("does not fit above the stack; clamped")), "{:?}", info.warnings);
}

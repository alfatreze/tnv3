//! The subtitle reader against the repository's vector files and against areas made by the native builder.
use std::{fs::File, path::PathBuf};
use tau_video::{
    Tnv3Reader,
    subtitle_read::{GRID_H, GRID_W, GlyphRef, parse_subtitle_area},
};

fn vectors() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/vectors/tnv3_15_16")
}

#[test]
fn valid_vectors_are_read_with_their_outline_widths_and_bad_ones_are_refused() {
    let text = std::fs::read_to_string(vectors().join("expected.txt")).expect("vector list");
    let mut read = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let (name, verdict) = (it.next().unwrap(), it.next().unwrap());
        let file = File::open(vectors().join(format!("{name}.tnv3"))).unwrap();
        let outcome = Tnv3Reader::open(file).and_then(|mut r| r.read_subtitle_area());
        if verdict == "invalid" {
            assert!(outcome.is_err(), "{name} must be refused by open or by the area reader");
            continue;
        }
        let area = outcome.unwrap().unwrap_or_else(|| panic!("{name} has a subtitle area"));
        read += 1;
        let widths: Vec<u8> = area.tracks[0].variants.iter().map(|v| v.outline_w).collect();
        match name {
            "ok15_w0_draft14_compatible" => assert_eq!(widths, [0]),
            "ok15_w1_w2_two_sizes" => assert_eq!(widths, [1, 2]),
            "ok15_w4_largest" => assert_eq!(widths, [4]),
            _ => assert_eq!(widths, [1]),
        }
        for v in &area.tracks[0].variants {
            assert!(v.glyphs.iter().all(|g| g.outline.is_some() == (v.outline_w > 0)));
            assert_eq!(v.glyphs.len(), 3);
        }
    }
    assert_eq!(read, 6);
}

#[test]
fn bottom_exact_vector_draws_inside_the_grid() {
    let file = File::open(vectors().join("ok15_two_lines_bottom_exact.tnv3")).unwrap();
    let area = Tnv3Reader::open(file).unwrap().read_subtitle_area().unwrap().unwrap();
    let v = &area.tracks[0].variants[0];
    let f = v.render(100);
    let rows: Vec<usize> = (0..GRID_H).filter(|&y| (0..GRID_W).any(|x| f.text[y * GRID_W + x] > 0)).collect();
    assert!(!rows.is_empty() && *rows.last().unwrap() < GRID_H);
    assert!(rows[0] >= 720 - 16, "text sits in the last two cell rows: {rows:?}");
}

#[cfg(feature = "subtitle-builder")]
mod with_builder {
    use super::*;
    use tau_video::subtitle::{self, Options, TrackInput, default_fonts};

    const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello <i>there</i>\n\n2\n00:00:03,000 --> 00:00:04,000\n[door slams]\nSecond line\n";

    #[test]
    fn builder_output_round_trips_through_the_reader() {
        let track = TrackInput { name: "a.srt".into(), text: SRT.into(), language: "eng".into(), default: true, forced: false };
        let mut o = Options::new(vec![track], default_fonts(), (400, 224));
        o.fps = Some((24, 1));
        o.cells = vec![(16, 24), (20, 28)];
        let (bytes, info) = subtitle::build_area(&o).unwrap();
        let area = parse_subtitle_area(&bytes).unwrap();
        let t = &area.tracks[0];
        assert_eq!((t.language.as_str(), t.default, t.variants.len()), ("eng", true, 2));
        for (v, vi) in t.variants.iter().zip(&info.tracks[0].variants) {
            assert_eq!((v.cell_w as u32, v.cell_h as u32), vi.cell);
            assert_eq!(v.events.len(), vi.events_out);
            assert_eq!(v.glyphs.len(), vi.glyphs);
            assert_eq!(v.outline_w, 2, "builder default outline");
            assert_eq!(v.events.iter().filter(|e| e.sdh).count(), vi.sdh_events);
            let switches = v.events.iter().flat_map(|e| &e.refs).filter(|r| matches!(r, GlyphRef::Colour(_))).count();
            assert_eq!(switches, vi.switches);
        }
        // the first event (1.0 s .. 2.0 s snapped to pictures) draws text and an outline; nothing at 2.5 s
        let v = &t.variants[0];
        let e = &v.events[0];
        assert_eq!((e.start, e.end), (48_000, 96_000));
        let f = v.render(e.start);
        assert!(f.text.iter().any(|&a| a > 0) && f.outline.iter().any(|&a| a > 0));
        assert!(v.render(120_000).text.iter().all(|&a| a == 0), "gap between the two events");
        // text pixels stay within the event's cell rows
        let ys: Vec<usize> = (0..GRID_H).filter(|&y| (0..GRID_W).any(|x| f.text[y * GRID_W + x] > 0)).collect();
        assert!(ys[0] >= usize::from(e.y) && *ys.last().unwrap() < usize::from(e.y) + usize::from(v.cell_h));
    }
}

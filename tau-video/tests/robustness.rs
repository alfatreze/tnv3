//! Deterministic mutation testing: no input may make the readers, the muxer or the subtitle parsers panic.
//! Valid seeds (this repository's vectors, a muxed film with audio and subtitles) get single-byte flips,
//! multi-byte noise, truncations, splices and zero/0xFF runs from a fixed-seed generator; every result is
//! allowed to be an error but never a panic. `ROBUSTNESS_ROUNDS` raises the number of mutations per seed
//! (default 150). Case count and the failing seed are printed on failure.
use std::{
    io::Cursor,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};
use tau_video::{
    Tnv3Reader, check_chunk_structure, check_container_preview,
    container::{MuxOptions, extract_tvc, mux_streams},
    subtitle_read::parse_subtitle_area,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut d = seed.to_vec();
    if d.is_empty() {
        return d;
    }
    match rng.below(6) {
        0 => {
            let i = rng.below(d.len());
            d[i] ^= 1 << rng.below(8);
        }
        1 => {
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(d.len());
                d[i] = rng.next() as u8;
            }
        }
        2 => d.truncate(rng.below(d.len())),
        3 => {
            let (a, n) = (rng.below(d.len()), 1 + rng.below(64));
            let fill = if rng.below(2) == 0 { 0 } else { 0xFF };
            for b in d.iter_mut().skip(a).take(n) {
                *b = fill;
            }
        }
        4 => {
            // header-biased: the first 64 bytes carry every offset and length
            let i = rng.below(64.min(d.len()));
            d[i] = rng.next() as u8;
        }
        _ => {
            let (a, b) = (rng.below(d.len()), rng.below(d.len()));
            let n = 1 + rng.below(32);
            let chunk: Vec<u8> = d.iter().skip(a).take(n).copied().collect();
            for (k, v) in chunk.into_iter().enumerate() {
                if b + k < d.len() {
                    d[b + k] = v;
                }
            }
        }
    }
    d
}

fn rounds() -> usize {
    std::env::var("ROBUSTNESS_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(150)
}

fn vector_files() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/vectors/tnv3_15_16");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).expect("vector directory") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "tnv3") {
            out.push((p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()));
        }
    }
    out.sort();
    assert!(out.len() >= 18);
    out
}

/// Runs everything a consumer would run over one container image.
fn exercise_container(bytes: &[u8]) {
    let Ok(mut r) = Tnv3Reader::open(Cursor::new(bytes)) else { return };
    let info = r.info().clone();
    let _ = check_container_preview(&mut r);
    if let Ok(mut r2) = Tnv3Reader::open(Cursor::new(bytes)) {
        let _ = check_chunk_structure(&mut r2);
        let _ = r2.read_subtitle_area();
        for n in 0..info.frame_count.min(64) {
            let _ = r2.index_entry(n);
            let _ = r2.read_picture(n);
        }
    }
    let _ = extract_tvc(Cursor::new(bytes));
}

#[test]
fn mutated_containers_never_panic() {
    let mut cases = 0usize;
    for (name, seed) in vector_files() {
        let mut rng = Rng(0x9E3779B97F4A7C15 ^ seed.len() as u64 ^ name.len() as u64);
        for round in 0..rounds() {
            let m = mutate(&seed, &mut rng);
            cases += 1;
            let r = catch_unwind(AssertUnwindSafe(|| exercise_container(&m)));
            assert!(r.is_ok(), "panic on {name}, round {round} (len {})", m.len());
        }
    }
    assert!(cases >= 18 * rounds());
}

#[test]
fn mutated_subtitle_areas_never_panic() {
    let mut cases = 0usize;
    for (name, seed) in vector_files() {
        let so = u32::from_le_bytes(seed[44..48].try_into().unwrap()) as usize;
        let sl = u32::from_le_bytes(seed[48..52].try_into().unwrap()) as usize;
        if sl == 0 || so + sl > seed.len() {
            continue;
        }
        let area = &seed[so..so + sl];
        let mut rng = Rng(0xD1B54A32D192ED03 ^ name.len() as u64);
        for round in 0..rounds() * 4 {
            let m = mutate(area, &mut rng);
            cases += 1;
            let r = catch_unwind(AssertUnwindSafe(|| {
                if let Ok(a) = parse_subtitle_area(&m) {
                    for t in a.tracks.iter().flat_map(|t| &t.variants) {
                        let _ = t.render(t.events.first().map_or(0, |e| e.start));
                    }
                }
            }));
            assert!(r.is_ok(), "panic on the area of {name}, round {round}");
        }
    }
    assert!(cases > 100);
}

#[test]
fn mutated_mux_inputs_never_panic() {
    // video: the TVC1 stream of a vector; audio: a small hand-made TAC0 file; subtitles: a vector's area
    let (_, base) = vector_files().into_iter().find(|(n, _)| n == "ok15_w1_w2_two_sizes.tnv3").unwrap();
    let video = extract_tvc(Cursor::new(&base)).unwrap();
    let so = u32::from_le_bytes(base[44..48].try_into().unwrap()) as usize;
    let sl = u32::from_le_bytes(base[48..52].try_into().unwrap()) as usize;
    let area = base[so..so + sl].to_vec();
    let mut tac = b"TAC0".to_vec();
    tac.extend(48000u32.to_le_bytes());
    tac.extend([2, 0, 0, 0]);
    tac.extend(9u32.to_le_bytes()); // frames: ceil(8192 / 1024) + 1, as a real encoder writes
    tac.extend(8192u32.to_le_bytes());
    for i in 0..9u8 {
        tac.extend(6u16.to_le_bytes());
        tac.extend([i, 0x7F, 0xFF, 0xFF, 0xFF, 0]);
    }
    let plain = MuxOptions { audio: Some(&tac), subtitles: Some(&area), trim_audio: true, ..Default::default() };
    mux_streams(&video, &plain, Cursor::new(Vec::new())).expect("the unmutated inputs must mux");
    let mut rng = Rng(0xA0761D6478BD642F);
    let mut ok = 0;
    for round in 0..rounds() * 6 {
        let (v, a, s) = match rng.below(3) {
            0 => (mutate(&video, &mut rng), tac.clone(), area.clone()),
            1 => (video.clone(), mutate(&tac, &mut rng), area.clone()),
            _ => (video.clone(), tac.clone(), mutate(&area, &mut rng)),
        };
        let opts = MuxOptions {
            audio: Some(&a),
            subtitles: Some(&s),
            crc: rng.below(2) == 0,
            level: [None, Some(1), Some(2), Some(3), Some(0), Some(9)][rng.below(6)],
            rate: [None, Some((24, 1)), Some((0, 1)), Some((1, 0)), Some((60000, 1))][rng.below(5)],
            trim_audio: rng.below(2) == 0,
            audio_shift: rng.below(9) as i32 - 4,
        };
        let r = catch_unwind(AssertUnwindSafe(|| mux_streams(&v, &opts, Cursor::new(Vec::new())).is_ok()));
        match r {
            Ok(true) => ok += 1,
            Ok(false) => {}
            Err(_) => panic!("panic in mux_streams, round {round}"),
        }
    }
    assert!(ok > 0, "some unmutated-enough inputs must still mux");
}

#[cfg(feature = "subtitle-builder")]
#[test]
fn random_subtitle_text_never_panics_the_builder() {
    use tau_video::subtitle::{self, Options, TrackInput, default_fonts};
    let seeds = [
        "1\n00:00:01,000 --> 00:00:02,000\nHello <i>there</i> &amp; &#x1F600; {\\i1}x{\\i0}\n\n2\n00:00:02,500 --> 00:00:03,000\n[sound]\n",
        "[V4+ Styles]\nFormat: Name, Fontname, PrimaryColour, Bold, Italic\nStyle: Default,Arial,&H00FFFFFF,0,0\n[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,Plain {\\c&H0000FF&}red{\\c} {\\fnX}f\\Nbreak\n",
    ];
    let mut rng = Rng(0xE7037ED1A0B428DB);
    let mut built = 0;
    for (k, seed) in seeds.iter().enumerate() {
        for _ in 0..rounds() {
            let bytes = mutate(seed.as_bytes(), &mut rng);
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let name = if k == 0 { "x.srt" } else { "x.ass" };
            let mut o = Options::new(
                vec![TrackInput { name: name.into(), text, language: "eng".into(), default: false, forced: false, quiet_font_fallback: true }],
                default_fonts(),
                (400, 224),
            );
            o.cells = vec![(16, 24)];
            let r = catch_unwind(AssertUnwindSafe(|| subtitle::build_area(&o).is_ok()));
            match r {
                Ok(true) => built += 1,
                Ok(false) => {}
                Err(_) => panic!("panic in the subtitle builder for mutated {name}: {:?}", seed.chars().take(20).collect::<String>()),
            }
        }
    }
    assert!(built > 0);
}

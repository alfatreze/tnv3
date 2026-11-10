//! Pure-Rust subtitle area builder (TNV3 container section 6, drafts 1.4 to 1.6): SRT / ASS tracks and fonts in,
//! one subtitle area out. It is a port of Tau V's reference builder `tools/tst_ref/tst_build.py` (layout,
//! wrapping, stacking, palette, SDH and forced flags, baked outline, packing) with `ab_glyph` for glyph
//! rasterisation instead of Pillow, so it needs no Python. Output is structurally the same as the
//! reference's but not byte-identical: outlines are unhinted, so glyph pixels (and rarely a chosen font
//! size or an advance) can differ. The area is checked with this crate's own subtitle validator before
//! it is returned.
//!
//! Known gaps against the reference: ASS positioning and size overrides are ignored (as there); HTML
//! entity and Unicode category handling are approximations (see `text.rs`); the placement case in which a
//! stack does not fit above row 0 is reported as a warning and clamped to row 0, exactly like Tau V's tool
//! (their known defect), not fixed.
mod build;
mod font;
mod text;

pub use build::{BuildInfo, TrackInfo, VariantInfo};
pub use font::{FontSpec, Style};

use std::{collections::BTreeMap, fmt};

/// Failure of the builder.
#[derive(Debug)]
pub struct Error(pub String);
impl Error {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        Error(s.into())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

/// A subtitle track's text (SRT, or ASS/SSA when `name` ends in `.ass` or `.ssa`).
#[derive(Clone, Debug)]
pub struct TrackInput {
    /// File name, used for the format decision and in messages.
    pub name: String,
    /// Decoded text (UTF-8; a BOM is removed).
    pub text: String,
    /// Two to four lower-case ASCII letters; `und` if unknown.
    pub language: String,
    pub default: bool,
    pub forced: bool,
    /// Do not warn when a font named in the text (for example the `Arial` of an extracted ASS style) is not
    /// among the given fonts and the default font is used instead.
    pub quiet_font_fallback: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// One event per line, centred (default).
    Center,
    /// Lines with equal SDH state become one event with line breaks.
    Block,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Auto,
    Bar,
    Picture,
}

/// Speaker style sheet entry: a source line starting with the prefix is restyled.
#[derive(Clone, Debug, Default)]
pub struct SpeakerStyle {
    pub font: Option<String>,
    /// `RRGGBB` or a colour name.
    pub color: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

/// Called with the fraction of the build done, 0..=1, from the thread that builds (rising, not exact).
pub type ProgressFn = std::sync::Arc<dyn Fn(f64) + Send + Sync>;

#[derive(Clone)]
pub struct Options {
    pub tracks: Vec<TrackInput>,
    pub fonts: Vec<FontSpec>,
    /// Cell sizes (width, height) with strictly increasing height.
    pub cells: Vec<(u32, u32)>,
    /// Picture size (it is centred vertically in the 800x720 grid).
    pub video: (u32, u32),
    /// Snap event times to picture boundaries.
    pub fps: Option<(u32, u32)>,
    /// Base font size in grid pixels (single size only); `None` = the largest that fits.
    pub font_size: Option<f64>,
    pub margin: u32,
    pub bottom: u32,
    pub align: Align,
    pub placement: Placement,
    /// Set the SDH flag automatically.
    pub sdh: bool,
    /// Prefix -> style (sorted by prefix).
    pub styles: BTreeMap<String, SpeakerStyle>,
    /// Colour for italics when the family has no italic face.
    pub italic_rgb: (u8, u8, u8),
    /// Centre a lone single line in the space of two (default on).
    pub center_single: bool,
    /// Baked outline radius 0..4: one value for every size, or one per size.
    pub outline_widths: Vec<u8>,
    /// Progress callback (optional).
    pub progress: Option<ProgressFn>,
}
/// Size variants Tau V settled on after watching on the Pocket (their interop messages 001 F-004 and 002 F-009;
/// the first test sizes 16x24, 20x28 and 26x36 were judged too small): cell sizes and the baked outline width of each.
pub const POCKET_CELLS: [(u32, u32); 4] = [(32, 44), (40, 54), (48, 64), (56, 76)];
pub const POCKET_OUTLINES: [u8; 4] = [2, 3, 3, 4];
/// Above this size the Tau V Pocket player may refuse the area (informative; the format itself allows far more).
pub const POCKET_AREA_LIMIT: usize = 1_000_000;

impl Options {
    /// The Pocket defaults: the four sizes of [`POCKET_CELLS`] with outlines [`POCKET_OUTLINES`] and `bar`
    /// placement (text in the bottom bar with upward stacking; `picture` placement cannot fit sizes this large).
    pub fn pocket(tracks: Vec<TrackInput>, fonts: Vec<FontSpec>, video: (u32, u32)) -> Self {
        let mut o = Self::new(tracks, fonts, video);
        o.cells = POCKET_CELLS.to_vec();
        o.outline_widths = POCKET_OUTLINES.to_vec();
        o.placement = Placement::Bar;
        o
    }

    /// Defaults of the reference builder for the given inputs.
    pub fn new(tracks: Vec<TrackInput>, fonts: Vec<FontSpec>, video: (u32, u32)) -> Self {
        Options {
            tracks,
            fonts,
            cells: vec![(20, 28)],
            video,
            fps: None,
            font_size: None,
            margin: 40,
            bottom: 24,
            align: Align::Center,
            placement: Placement::Auto,
            sdh: true,
            styles: BTreeMap::new(),
            italic_rgb: (255, 224, 64),
            center_single: true,
            outline_widths: vec![2],
            progress: None,
        }
    }
}

/// The built-in default fonts: Roboto Regular as the default face with its Italic, Bold and Bold Italic
/// (SIL Open Font License 1.1, `LICENSES/OFL-Roboto.txt`). Use them when the caller gives no font.
pub fn default_fonts() -> Vec<FontSpec> {
    let face = |style: Style, data: &[u8], label: &str| FontSpec {
        family: String::new(),
        style,
        data: data.to_vec(),
        label: format!("{label} (built in)"),
        scale: None,
    };
    vec![
        face(Style::Regular, include_bytes!("../../assets/fonts/Roboto-Regular.ttf"), "Roboto-Regular.ttf"),
        face(Style::Italic, include_bytes!("../../assets/fonts/Roboto-Italic.ttf"), "Roboto-Italic.ttf"),
        face(Style::Bold, include_bytes!("../../assets/fonts/Roboto-Bold.ttf"), "Roboto-Bold.ttf"),
        face(Style::BoldItalic, include_bytes!("../../assets/fonts/Roboto-BoldItalic.ttf"), "Roboto-BoldItalic.ttf"),
    ]
}

/// Builds the subtitle area. Warnings (placement clamps, clipped glyphs, missing characters, ...) are
/// returned in [`BuildInfo::warnings`], never dropped.
pub fn build_area(o: &Options) -> Result<(Vec<u8>, BuildInfo), Error> {
    build::build_area(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ass_styles_colours_and_italics_survive_and_font_fallback_can_be_quiet() {
        let ass = "[V4+ Styles]\nFormat: Name, Fontname, PrimaryColour, Bold, Italic\nStyle: Default,Arial,&H00FFFFFF,0,0\nStyle: Yellow,Arial,&H0000E6FF,0,0\n[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,Plain {\\i1}it{\\i0} {\\c&H0000FF&}red{\\c}\nDialogue: 0,0:00:03.00,0:00:04.00,Yellow,{\\an8}Yellow\n";
        let make = |quiet: bool| {
            Options::new(
                vec![TrackInput { name: "x.ass".into(), text: ass.into(), language: "eng".into(), default: false, forced: false, quiet_font_fallback: quiet }],
                default_fonts(),
                (400, 224),
            )
        };
        let (bytes, loud) = build_area(&make(false)).unwrap();
        assert!(loud.warnings.iter().any(|w| w.contains("Arial")), "unknown style font reported: {:?}", loud.warnings);
        let (_, quiet) = build_area(&make(true)).unwrap();
        assert!(quiet.warnings.is_empty(), "{:?}", quiet.warnings);
        let area = crate::subtitle_read::parse_subtitle_area(&bytes).unwrap();
        let pal = &area.tracks[0].palette;
        assert_eq!((pal[0], pal[1], pal[2]), ([255, 255, 255], [255, 0, 0], [255, 230, 0]), "default, inline red, Yellow style");
        let ev = &area.tracks[0].variants[0].events;
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1].colour, 2, "the second event starts in the Yellow style colour");
    }
    #[test]
    fn progress_callback_rises_to_the_end() {
        use std::sync::{Arc, Mutex};
        let mut text = String::from("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
        for i in 0..40 {
            text.push_str(&format!("Dialogue: 0,0:00:{:02}.00,0:00:{:02}.50,Default,,0,0,0,,Line number {i}\n", i % 59, i % 59));
        }
        let seen = Arc::new(Mutex::new(Vec::<f64>::new()));
        let sink = seen.clone();
        let mut o = Options::pocket(
            vec![TrackInput { name: "x.ass".into(), text, language: "eng".into(), default: true, forced: false, quiet_font_fallback: true }],
            default_fonts(),
            (400, 224),
        );
        o.progress = Some(Arc::new(move |f| sink.lock().unwrap().push(f)));
        build_area(&o).unwrap();
        let v = seen.lock().unwrap().clone();
        assert!(v.len() >= 8 && v.first() == Some(&0.0), "{v:?}");
        assert!(v.windows(2).all(|w| w[1] >= w[0] - 1e-9), "never goes back: {v:?}");
        assert!(*v.last().unwrap() >= 0.98 && v.iter().all(|f| (0.0..=1.0).contains(f)), "{v:?}");
    }
    #[test]
    fn bold_and_bold_italic_styles_use_their_own_faces() {
        let ass = "[V4+ Styles]\nFormat: Name, Fontname, PrimaryColour, Bold, Italic\nStyle: Default,Roboto,&H00FFFFFF,0,0\nStyle: Strong,Roboto,&H00FFFFFF,-1,0\n[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,Plain\nDialogue: 0,0:00:03.00,0:00:04.00,Strong,Bold {\\i1}both{\\i0}\n";
        let mut o = Options::new(
            vec![TrackInput { name: "x.ass".into(), text: ass.into(), language: "eng".into(), default: false, forced: false, quiet_font_fallback: true }],
            default_fonts(),
            (400, 224),
        );
        o.cells = vec![(20, 28)];
        let (_, info) = build_area(&o).unwrap();
        assert!(info.warnings.is_empty(), "{:?}", info.warnings);
        assert_eq!(info.tracks[0].variants[0].faces, 3, "regular, bold and bold italic");
    }
    #[test]
    fn built_in_fonts_load_and_cover_basic_latin() {
        let fonts = default_fonts();
        assert_eq!(fonts.len(), 4);
        let (_, info) = build_area(&Options::new(
            vec![TrackInput { name: "a.srt".into(), text: "1\n00:00:01,000 --> 00:00:02,000\nHello <i>there</i> \u{e9}\n".into(), language: "eng".into(), default: false, forced: false, quiet_font_fallback: false }],
            fonts,
            (400, 224),
        ))
        .unwrap();
        assert!(info.warnings.is_empty(), "{:?}", info.warnings);
        assert_eq!(info.tracks[0].variants[0].faces, 2, "regular and italic faces both used (bold is only loaded when asked for)");
    }
    #[test]
    fn font_argument_forms_are_parsed() {
        let dir = std::env::temp_dir().join(format!("tau-font-arg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.ttf");
        std::fs::write(&f, b"not a font").unwrap();
        let p = f.display().to_string();
        let a = FontSpec::from_argument(&p).unwrap();
        assert_eq!((a.family.as_str(), a.style, a.scale), ("", Style::Regular, None));
        let b = FontSpec::from_argument(&format!("demon:bold={p}@1.5")).unwrap();
        assert_eq!((b.family.as_str(), b.style, b.scale), ("demon", Style::Bold, Some(1.5)));
        assert!(FontSpec::from_argument(&format!("x:fancy={p}")).is_err());
        assert!(FontSpec::from_argument("/no/such.ttf").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

//! Encode progress drawn on the application icon (macOS Dock): a segmented bar over the icon's lower edge,
//! video 6/8, audio 1/8, subtitles 1/8 (without subtitles video 6/7 and audio 1/7). Video is determinate (a bright head
//! marks the edge); audio and subtitles cannot report progress, so while one of them runs its segment shimmers.
//! The bar is amber, deliberately unlike the icon's green. Frames are composed here in plain Rust (testable on
//! any platform) and handed to AppKit as PNG data; elsewhere the OS-native progress bar is used instead.
use std::{
    f32::consts::PI,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return Err("the icon must be an RGB or RGBA PNG".into()),
    };
    Ok(Image { w: info.width, h: info.height, rgba })
}

pub fn encode_png(img: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w, img.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header().and_then(|mut w| w.write_image_data(&img.rgba)).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// What the icon shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconPhase {
    /// The plain icon.
    Idle,
    /// Reading the source: the whole bar shimmers.
    Probing,
    /// Video encoding; `Some(fraction)` 0..=1, or `None` when the total is unknown (shimmer in the video segment).
    Video(Option<f32>),
    /// Video encoding with the audio encoded beside it: the video fraction and the audio fraction (0..=1).
    VideoAudio(Option<f32>, f32),
    /// The audio step; `Some(fraction)` 0..=1, or `None` when its progress is unknown (shimmer).
    Audio(Option<f32>),
    /// The subtitle step, likewise.
    Subtitles(Option<f32>),
    /// Writing the file: every segment full, a shimmer runs along the whole bar.
    Saving,
    Done,
    Failed,
}

const AMBER: [f32; 3] = [255.0, 176.0, 32.0];
const HOT: [f32; 3] = [255.0, 240.0, 190.0];
const TRACK: [f32; 3] = [58.0, 64.0, 72.0];
const DONE: [f32; 3] = [88.0, 222.0, 150.0];
const FAILED: [f32; 3] = [255.0, 84.0, 84.0];

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Signed distance to a capsule (a pill) centred at (cx, cy) with half length `half_w` and radius `r`.
fn capsule(x: f32, y: f32, cx: f32, cy: f32, half_w: f32, r: f32) -> f32 {
    let dx = ((x - cx).abs() - (half_w - r)).max(0.0);
    let dy = y - cy;
    (dx * dx + dy * dy).sqrt() - r
}

/// Geometry of the segments: (left, right) in pixels for video, audio and (optionally) subtitles.
pub fn segments(size: u32, has_subs: bool) -> Vec<(f32, f32)> {
    let s = size as f32;
    let (x0, x1, gap) = (0.11 * s, 0.89 * s, 0.014 * s);
    let weights: &[f32] = if has_subs { &[6.0, 1.0, 1.0] } else { &[6.0, 1.0] };
    let total: f32 = weights.iter().sum();
    let room = (x1 - x0) - gap * (weights.len() as f32 - 1.0);
    let mut out = Vec::new();
    let mut x = x0;
    for w in weights {
        let width = room * w / total;
        out.push((x, x + width));
        x += width + gap;
    }
    out
}

/// A bright band moving left to right across `u` in 0..=1, one pass per `period_ms`, with a short pause.
fn shimmer(u: f32, t_ms: u64, period_ms: f32) -> f32 {
    let phase = (t_ms as f32 % period_ms) / period_ms * 1.5 - 0.25;
    (-((u - phase) / 0.16).powi(2)).exp()
}

/// Composes one frame: `base` (the icon) with the bar for `phase` at time `t_ms` (drives the animation).
pub fn compose(base: &Image, phase: IconPhase, has_subs: bool, t_ms: u64) -> Image {
    let mut img = base.clone();
    if phase == IconPhase::Idle {
        return img;
    }
    let size = base.w.min(base.h);
    let s = size as f32;
    let segs = segments(size, has_subs);
    let (bar_l, bar_r) = (segs[0].0, segs[segs.len() - 1].1);
    let (cy, bar_h) = (0.885 * s, 0.075 * s);
    let r = bar_h / 2.0;
    let pulse = 0.5 + 0.5 * (t_ms as f32 / 1000.0 * 2.0 * PI * 1.2).sin();
    // per segment: (fill fraction 0..=1, active shimmer in this segment, colour)
    let n = segs.len();
    let mut fill = vec![0.0f32; n];
    let mut busy = vec![false; n];
    let mut hot_head = None::<usize>;
    let mut global_shimmer = false;
    let mut colour = AMBER;
    match phase {
        IconPhase::Idle => {}
        IconPhase::Probing => global_shimmer = true,
        IconPhase::Video(Some(f)) => {
            fill[0] = f.clamp(0.0, 1.0);
            hot_head = Some(0);
        }
        IconPhase::Video(None) => busy[0] = true,
        IconPhase::VideoAudio(v, a) => {
            match v {
                Some(f) => {
                    fill[0] = f.clamp(0.0, 1.0);
                    hot_head = Some(0);
                }
                None => busy[0] = true,
            }
            fill[1] = a.clamp(0.0, 1.0);
        }
        IconPhase::Audio(a) => {
            fill[0] = 1.0;
            match a {
                Some(f) => {
                    fill[1] = f.clamp(0.0, 1.0);
                    hot_head = Some(1);
                }
                None => busy[1] = true,
            }
        }
        IconPhase::Subtitles(s) => {
            fill[0] = 1.0;
            fill[1] = 1.0;
            if n > 2 {
                match s {
                    Some(f) => {
                        fill[2] = f.clamp(0.0, 1.0);
                        hot_head = Some(2);
                    }
                    None => busy[2] = true,
                }
            }
        }
        IconPhase::Saving => {
            fill.iter_mut().for_each(|f| *f = 1.0);
            global_shimmer = true;
        }
        IconPhase::Done => {
            fill.iter_mut().for_each(|f| *f = 1.0);
            colour = DONE;
        }
        IconPhase::Failed => {
            fill.iter_mut().for_each(|f| *f = 1.0);
            colour = FAILED;
        }
    }
    let channels = img.rgba.len() / (img.w as usize * img.h as usize);
    debug_assert_eq!(channels, 4);
    for py in 0..img.h {
        for px in 0..img.w {
            let i = (py * img.w + px) as usize * 4;
            let base_a = f32::from(img.rgba[i + 3]) / 255.0;
            if base_a <= 0.0 {
                continue;
            }
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let mut c = [f32::from(img.rgba[i]), f32::from(img.rgba[i + 1]), f32::from(img.rgba[i + 2])];
            // scrim: darken the lower part so the bar stands out from the icon
            let scrim = ((y / s - 0.70) / 0.30).clamp(0.0, 1.0).powf(1.2) * 0.6;
            c = lerp(c, [0.0, 0.0, 0.0], scrim);
            for (k, &(l, rr)) in segs.iter().enumerate() {
                let (cx, half) = ((l + rr) / 2.0, (rr - l) / 2.0);
                let d = capsule(x, y, cx, cy, half, r);
                let cover = (0.5 - d).clamp(0.0, 1.0);
                let active = fill[k] > 0.0 || busy[k] || global_shimmer || matches!(phase, IconPhase::Done | IconPhase::Failed);
                // halo around active segments
                if active && d > 0.0 && d < 0.05 * s {
                    let halo = (1.0 - d / (0.05 * s)).powi(2) * 0.38 * (0.75 + 0.25 * pulse);
                    c = lerp(c, colour, halo);
                }
                if cover <= 0.0 {
                    continue;
                }
                // empty track with a lighter rim so unfilled segments stay visible
                let rim = (1.0 - ((-d) / (0.012 * s)).clamp(0.0, 1.0)) * cover;
                c = lerp(c, TRACK, 0.92 * cover);
                c = lerp(c, [120.0, 128.0, 138.0], 0.55 * rim);
                let u = ((x - l) / (rr - l)).clamp(0.0, 1.0);
                let g = ((x - bar_l) / (bar_r - bar_l)).clamp(0.0, 1.0);
                let edge = l + (rr - l) * fill[k];
                let lit = if fill[k] >= 1.0 { 1.0 } else { ((edge - x) + 0.5).clamp(0.0, 1.0) * f32::from(fill[k] > 0.0) };
                let mut col = colour;
                let mut alpha = lit;
                if busy[k] {
                    // an unknown-length step: dim base, a pulse, and a band that runs along the segment
                    let band = shimmer(u, t_ms, 1100.0);
                    alpha = alpha.max(0.30 + 0.12 * pulse + 0.58 * band);
                    col = lerp(colour, HOT, band * 0.85);
                } else if global_shimmer {
                    let band = shimmer(g, t_ms, 1500.0);
                    if lit < 1.0 {
                        alpha = alpha.max(0.30 + 0.12 * pulse + 0.58 * band);
                    } else {
                        col = lerp(colour, HOT, band * 0.9);
                    }
                    if lit >= 1.0 && band > 0.0 {
                        alpha = 1.0;
                    }
                }
                if hot_head == Some(k) && fill[k] > 0.0 && fill[k] < 1.0 {
                    let head = (-((x - edge) / (0.03 * s)).powi(2)).exp();
                    col = lerp(col, HOT, head * (0.55 + 0.35 * pulse));
                }
                c = lerp(c, col, alpha.clamp(0.0, 1.0) * cover);
            }
            for (k, v) in c.iter().enumerate() {
                img.rgba[i + k] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    img
}

// ---------------------------------------------------------------------------------------------- live icon

struct Live {
    phase: IconPhase,
    until: Option<Instant>,
}

/// Owns the animation of the Dock icon for one encode. Dropping it restores the normal icon.
pub struct DockIcon {
    live: Arc<Mutex<Live>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl DockIcon {
    /// Starts the animation (macOS only; on other systems this does nothing and the caller uses the native bar).
    pub fn start(app: &tauri::AppHandle, has_subs: bool) -> Self {
        let live = Arc::new(Mutex::new(Live { phase: IconPhase::Probing, until: None }));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = platform::spawn(app.clone(), live.clone(), stop.clone(), has_subs);
        DockIcon { live, stop, thread: Mutex::new(handle) }
    }

    pub fn set(&self, phase: IconPhase) {
        if let Ok(mut l) = self.live.lock() {
            if l.until.is_none() {
                l.phase = phase;
            }
        }
    }

    /// Shows the end state for a moment (green on success, red on failure), then restores the icon.
    pub fn finish(&self, ok: bool) {
        if let Ok(mut l) = self.live.lock() {
            l.phase = if ok { IconPhase::Done } else { IconPhase::Failed };
            l.until = Some(Instant::now() + Duration::from_millis(if ok { 1400 } else { 1800 }));
        }
    }

    /// Ends the animation immediately and restores the icon.
    pub fn stop_now(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
}

impl Drop for DockIcon {
    fn drop(&mut self) {
        // Let a pending end state play out (the thread stops itself); otherwise stop now.
        let finishing = self.live.lock().map(|l| l.until.is_some()).unwrap_or(false);
        if !finishing {
            self.stop_now();
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    fn set_icon(png: Option<Vec<u8>>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        match png {
            Some(bytes) => {
                let data = NSData::with_bytes(&bytes);
                let image = NSImage::initWithData(NSImage::alloc(), &data);
                // SAFETY: called on the main thread with a valid image (or nil to restore the bundle icon).
                unsafe { app.setApplicationIconImage(image.as_deref()) };
            }
            None => unsafe { app.setApplicationIconImage(None) },
        }
    }

    pub fn spawn(app: tauri::AppHandle, live: Arc<Mutex<Live>>, stop: Arc<AtomicBool>, has_subs: bool) -> Option<std::thread::JoinHandle<()>> {
        let base = decode_png(include_bytes!("../icons/128x128@2x.png")).ok()?;
        std::thread::Builder::new()
            .name("tau-dock-icon".into())
            .spawn(move || {
                let started = Instant::now();
                loop {
                    let (phase, until) = match live.lock() {
                        Ok(l) => (l.phase, l.until),
                        Err(_) => break,
                    };
                    if stop.load(Ordering::Acquire) || until.is_some_and(|u| Instant::now() >= u) {
                        break;
                    }
                    let frame = compose(&base, phase, has_subs, started.elapsed().as_millis() as u64);
                    if let Ok(png) = encode_png(&frame) {
                        let _ = app.run_on_main_thread(move || set_icon(Some(png)));
                    }
                    std::thread::sleep(Duration::from_millis(80));
                }
                let _ = app.run_on_main_thread(|| set_icon(None));
            })
            .ok()
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    pub fn spawn(_: tauri::AppHandle, _: Arc<Mutex<Live>>, _: Arc<AtomicBool>, _: bool) -> Option<std::thread::JoinHandle<()>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Image {
        decode_png(include_bytes!("../icons/128x128@2x.png")).expect("the app icon decodes")
    }
    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.w + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
    }
    fn amberish(p: [u8; 4]) -> bool {
        p[0] > 200 && p[1] > 120 && p[2] < 140 && p[0] > p[2] + 100
    }

    #[test]
    fn segments_follow_the_three_quarter_one_eighth_one_eighth_layout() {
        let s = segments(256, true);
        let w: Vec<f32> = s.iter().map(|(l, r)| r - l).collect();
        assert!((w[0] / w[1] - 6.0).abs() < 0.01 && (w[1] - w[2]).abs() < 0.01, "{w:?}");
        let video_share = w[0] / w.iter().sum::<f32>();
        assert!((video_share - 0.75).abs() < 0.001, "video is three quarters of the bar: {video_share}");
        let two = segments(256, false);
        assert_eq!(two.len(), 2);
        assert!(s[2].1 <= 256.0 * 0.89 + 0.01 && s[0].0 >= 256.0 * 0.11 - 0.01, "the bar stays inside the tile margins");
    }

    #[test]
    fn idle_leaves_the_icon_untouched_and_transparent_corners_stay_transparent() {
        let b = base();
        assert_eq!(compose(&b, IconPhase::Idle, true, 0).rgba, b.rgba);
        let f = compose(&b, IconPhase::Video(Some(0.5)), true, 0);
        assert_eq!(px(&f, 0, 0)[3], 0, "corner pixel stays transparent");
        assert_eq!(f.rgba.len(), b.rgba.len());
    }

    #[test]
    fn video_fill_grows_left_to_right_in_amber_and_other_segments_stay_dark() {
        let b = base();
        let s = segments(b.w, true);
        let y = (0.885 * b.w as f32) as u32;
        let at = |frac: f32, x: f32| px(&compose(&b, IconPhase::Video(Some(frac)), true, 0), x as u32, y);
        let mid = |k: usize| (s[k].0 + s[k].1) / 2.0;
        let quarter = s[0].0 + (s[0].1 - s[0].0) * 0.25;
        let three_quarters = s[0].0 + (s[0].1 - s[0].0) * 0.75;
        assert!(amberish(at(0.5, quarter)), "filled part is amber: {:?}", at(0.5, quarter));
        assert!(!amberish(at(0.5, three_quarters)), "unfilled part is not: {:?}", at(0.5, three_quarters));
        assert!(!amberish(at(0.5, mid(1))) && !amberish(at(0.5, mid(2))), "audio and subtitle segments are dark");
        assert!(amberish(at(1.0, three_quarters)), "a full video segment is amber end to end");
    }

    #[test]
    fn audio_and_subtitle_steps_shimmer_over_time() {
        let b = base();
        let s = segments(b.w, true);
        let y = (0.885 * b.w as f32) as u32;
        let sample = |phase, k: usize, t| {
            let f = compose(&b, phase, true, t);
            let x = (s[k].0 + (s[k].1 - s[k].0) * 0.5) as u32;
            let p = px(&f, x, y);
            u32::from(p[0]) + u32::from(p[1])
        };
        for (phase, k) in [(IconPhase::Audio(None), 1usize), (IconPhase::Subtitles(None), 2)] {
            let values: Vec<u32> = (0..12).map(|i| sample(phase, k, i * 100)).collect();
            let (lo, hi) = (values.iter().min().unwrap(), values.iter().max().unwrap());
            assert!(hi - lo > 120, "{phase:?} segment must visibly animate: {values:?}");
        }
        // the earlier steps are shown complete while a later one runs
        let f = compose(&b, IconPhase::Subtitles(None), true, 0);
        assert!(amberish(px(&f, (s[0].0 + 30.0) as u32, y)) && amberish(px(&f, (s[1].0 + (s[1].1 - s[1].0) * 0.5) as u32, y)));
        // without subtitles the bar has two segments and the subtitle phase draws nothing extra
        let two = compose(&b, IconPhase::Subtitles(None), false, 0);
        assert_eq!(two.rgba.len(), b.rgba.len());
    }

    #[test]
    fn audio_and_subtitle_steps_fill_with_their_fraction() {
        let b = base();
        let s = segments(b.w, true);
        let y = (0.885 * b.w as f32) as u32;
        let at = |phase: IconPhase, k: usize, frac: f32| {
            let x = s[k].0 + (s[k].1 - s[k].0) * frac;
            px(&compose(&b, phase, true, 0), x as u32, y)
        };
        // audio at 50%: lit before the middle of its segment, dark after it; the video segment is complete
        assert!(amberish(at(IconPhase::Audio(Some(0.5)), 1, 0.25)) && !amberish(at(IconPhase::Audio(Some(0.5)), 1, 0.75)));
        assert!(amberish(at(IconPhase::Audio(Some(0.5)), 0, 0.5)));
        assert!(amberish(at(IconPhase::Subtitles(Some(0.5)), 2, 0.25)) && !amberish(at(IconPhase::Subtitles(Some(0.5)), 2, 0.75)));
        // audio running beside the video: both fill independently
        let both = IconPhase::VideoAudio(Some(0.5), 0.5);
        assert!(amberish(at(both, 0, 0.25)) && !amberish(at(both, 0, 0.75)));
        assert!(amberish(at(both, 1, 0.25)) && !amberish(at(both, 1, 0.75)));
    }

    #[test]
    fn end_states_are_green_and_red() {
        let b = base();
        let s = segments(b.w, true);
        let y = (0.885 * b.w as f32) as u32;
        let x = (s[0].0 + 40.0) as u32;
        let done = px(&compose(&b, IconPhase::Done, true, 0), x, y);
        let failed = px(&compose(&b, IconPhase::Failed, true, 0), x, y);
        assert!(done[1] > done[0] + 60 && done[1] > done[2] + 40, "{done:?}");
        assert!(failed[0] > failed[1] + 100 && failed[0] > failed[2] + 100, "{failed:?}");
    }

    #[test]
    fn png_round_trip_keeps_pixels() {
        let b = base();
        let f = compose(&b, IconPhase::Video(Some(0.3)), true, 123);
        let back = decode_png(&encode_png(&f).unwrap()).unwrap();
        assert_eq!((back.w, back.h, back.rgba), (f.w, f.h, f.rgba));
    }

    /// Writes a contact sheet of the frames to the path in `TAU_ICON_SHEET` (for looking at them); no-op otherwise.
    #[test]
    fn render_contact_sheet_when_asked() {
        let Some(path) = std::env::var_os("TAU_ICON_SHEET") else { return };
        let b = base();
        let frames: Vec<(IconPhase, bool, u64)> = vec![
            (IconPhase::Probing, true, 300),
            (IconPhase::Video(Some(0.0)), true, 0),
            (IconPhase::Video(Some(0.35)), true, 200),
            (IconPhase::Video(Some(0.85)), true, 500),
            (IconPhase::Audio(None), true, 150),
            (IconPhase::Audio(None), true, 650),
            (IconPhase::Subtitles(None), true, 300),
            (IconPhase::Saving, true, 700),
            (IconPhase::Done, true, 0),
            (IconPhase::Failed, true, 0),
            (IconPhase::Video(Some(0.6)), false, 0),
            (IconPhase::Audio(None), false, 400),
        ];
        let (cols, cell) = (4u32, 256u32);
        let rows = (frames.len() as u32).div_ceil(cols);
        let mut sheet = Image { w: cols * cell, h: rows * cell, rgba: vec![0; (cols * cell * rows * cell * 4) as usize] };
        for p in sheet.rgba.chunks_mut(4) {
            p.copy_from_slice(&[40, 44, 54, 255]);
        }
        for (n, (phase, subs, t)) in frames.iter().enumerate() {
            let f = compose(&b, *phase, *subs, *t);
            let (ox, oy) = ((n as u32 % cols) * cell, (n as u32 / cols) * cell);
            for y in 0..f.h {
                for x in 0..f.w {
                    let i = ((y * f.w + x) * 4) as usize;
                    let a = u32::from(f.rgba[i + 3]);
                    let j = (((oy + y) * sheet.w + ox + x) * 4) as usize;
                    for c in 0..3 {
                        let bg = u32::from(sheet.rgba[j + c]);
                        sheet.rgba[j + c] = ((u32::from(f.rgba[i + c]) * a + bg * (255 - a)) / 255) as u8;
                    }
                }
            }
        }
        std::fs::write(path, encode_png(&sheet).unwrap()).unwrap();
    }
}

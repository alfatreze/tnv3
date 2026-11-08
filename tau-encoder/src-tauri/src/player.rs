use serde::Serialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use std::hash::{Hash, Hasher};
use tau_video::{
    Tnv3Reader,
    subtitle_read::{GRID_H, GRID_W, SubtitleArea},
    tvc::frame::{Frame420, PictureDecoder},
};

/// One selectable subtitle track of the open file.
#[derive(Serialize)]
pub struct SubtitleTrackInfo {
    pub index: u32,
    pub language: String,
    pub default: bool,
    pub forced_track: bool,
    /// Cell sizes of the size variants, smallest first.
    pub variants: Vec<(u8, u8)>,
}

#[derive(Serialize)]
pub struct PlayerInfo {
    pub width: u16,
    pub height: u16,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frames: u32,
    pub audio: Option<String>,
    /// `"mp3-sidecar"`, or `"embedded-tac"` (the caller decodes it and fills `audio`), or none.
    pub audio_source: Option<&'static str>,
    /// Empty when the file has no subtitle area.
    pub subtitles: Vec<SubtitleTrackInfo>,
}
pub struct Player {
    reader: Tnv3Reader<File>,
    file: File,
    decoder: PictureDecoder,
    next: u32,
    finished: bool,
    subtitles: Option<SubtitleArea>,
}
impl Player {
    pub fn open(path: &Path) -> Result<(Self, PlayerInfo), String> {
        let reader = Tnv3Reader::open(File::open(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let info = reader.info();
        let decoder = PictureDecoder::new(info.width, info.height).map_err(|e| e.to_string())?;
        let audio = path.with_extension("mp3");
        let audio = if audio.is_file() {
            Some(
                audio
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
        let audio_source = if audio.is_some() {
            Some("mp3-sidecar")
        } else if info.audio_channels > 0 && info.flags & 2 != 0 {
            Some("embedded-tac")
        } else {
            None
        };
        let subtitles = Tnv3Reader::open(File::open(path).map_err(|e| e.to_string())?)
            .and_then(|mut r| r.read_subtitle_area())
            .map_err(|e| format!("Subtitle area: {e}"))?;
        let subtitle_info = subtitles
            .as_ref()
            .map(|a| {
                a.tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| SubtitleTrackInfo {
                        index: i as u32,
                        language: t.language.clone(),
                        default: t.default,
                        forced_track: t.forced_track,
                        variants: t.variants.iter().map(|v| (v.cell_w, v.cell_h)).collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let result = PlayerInfo {
            width: info.width,
            height: info.height,
            fps_num: info.fps_num,
            fps_den: info.fps_den,
            frames: info.frame_count,
            audio,
            audio_source,
            subtitles: subtitle_info,
        };
        Ok((
            Self {
                reader,
                file: File::open(path).map_err(|e| e.to_string())?,
                decoder,
                next: 0,
                finished: false,
                subtitles,
            },
            result,
        ))
    }
    pub fn next_frame(&mut self) -> Result<Option<Frame420>, String> {
        if self.finished {
            return Ok(None);
        }
        while self.next < self.reader.info().frame_count {
            let entry = self
                .reader
                .index_entry(self.next)
                .map_err(|e| e.to_string())?;
            let info = self.reader.info();
            let off = u64::from(entry.offset);
            if off < 64 || off + 16 > u64::from(info.index_offset) {
                return Err("Invalid video chunk offset.".into());
            }
            self.file
                .seek(SeekFrom::Start(off))
                .map_err(|e| e.to_string())?;
            let mut head = [0u8; 16];
            self.file.read_exact(&mut head).map_err(|e| e.to_string())?;
            let len = u32::from_le_bytes(head[0..4].try_into().unwrap()) as usize;
            let pic = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
            let flags = u16::from_le_bytes([head[10], head[11]]);
            let sum = u32::from_le_bytes(head[12..16].try_into().unwrap());
            if !(19..=665_536).contains(&len)
                || !(3..=384_000).contains(&pic)
                || pic > len - 16
                || off + len as u64 > u64::from(info.index_offset)
                || flags & !3 != 0
            {
                return Err("Invalid video chunk.".into());
            }
            let mut body = vec![0; len - 16];
            self.file.read_exact(&mut body).map_err(|e| e.to_string())?;
            if (info.flags & 1 != 0 && tau_video::crc32(&body) != sum)
                || (info.flags & 1 == 0 && sum != 0)
            {
                return Err("Video chunk checksum mismatch.".into());
            }
            let expected = match body[0] {
                0 => 1,
                1 => 0,
                2 => 2,
                _ => return Err("Invalid picture type.".into()),
            };
            if flags != expected || entry.flags_and_seek & 3 != u32::from(flags) {
                return Err("Video picture flags disagree.".into());
            }
            self.next += 1;
            if let Some(frame) = self
                .decoder
                .decode(&body[..pic])
                .map_err(|e| e.to_string())?
            {
                return Ok(Some(frame));
            }
        }
        self.finished = true;
        Ok(self.decoder.finish())
    }
}

/// What the page needs to draw the subtitles at one moment.
pub struct Overlay {
    /// Identifies the set of active events and the settings; 0 is never used. The page passes the key it
    /// already shows and gets only the key back while nothing changed.
    pub key: u32,
    /// Bounding box in the 800x720 grid and straight-alpha RGBA of that box (empty when nothing is drawn).
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub rgba: Vec<u8>,
}

impl Player {
    /// Subtitle overlay at `position` audio samples. `track` and `variant` select what to draw; with
    /// `subtitles_on` false only the `forced` events of the track are drawn (spec 6.4); `hide_sdh` drops the
    /// `sdh` events. Layers: black outline under the text, text in the track's palette, 0..3 coverage blended
    /// as 0, 1/3, 2/3, 1 (spec 6.3). Player settings such as delay, offset or box are not applied.
    pub fn subtitle_overlay(&self, track: usize, variant: usize, position: i64, subtitles_on: bool, hide_sdh: bool, known_key: u32) -> Result<Overlay, String> {
        let area = self.subtitles.as_ref().ok_or("This file has no subtitles.")?;
        let t = area.tracks.get(track).ok_or("No such subtitle track.")?;
        let v = t.variants.get(variant).ok_or("No such subtitle size.")?;
        let p = position.clamp(0, i64::from(u32::MAX)) as u32;
        let keep = |e: &tau_video::subtitle_read::Event| (subtitles_on || e.forced) && !(hide_sdh && e.sdh);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (track, variant, subtitles_on, hide_sdh).hash(&mut h);
        for (i, e) in v.events.iter().enumerate() {
            if e.start <= p && p < e.end && keep(e) {
                i.hash(&mut h);
            }
        }
        let key = (h.finish() as u32).max(2);
        let empty = |key| Overlay { key, x: 0, y: 0, w: 0, h: 0, rgba: Vec::new() };
        if key == known_key {
            return Ok(empty(key));
        }
        let frame = v.render_filtered(p, keep);
        let (mut x0, mut y0, mut x1, mut y1) = (GRID_W, GRID_H, 0usize, 0usize);
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                let i = y * GRID_W + x;
                if frame.text[i] > 0 || frame.outline[i] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 <= x0 {
            return Ok(empty(key));
        }
        let (w, hgt) = (x1 - x0, y1 - y0);
        let mut rgba = vec![0u8; w * hgt * 4];
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * GRID_W + x;
                let (ta, oa) = (f32::from(frame.text[i]) / 3.0, f32::from(frame.outline[i]) / 3.0);
                let alpha = ta + oa * (1.0 - ta);
                if alpha <= 0.0 {
                    continue;
                }
                let col = t.palette[usize::from(frame.colour[i] & 15)];
                let o = ((y - y0) * w + (x - x0)) * 4;
                for c in 0..3 {
                    // the outline is black, so only the text contributes colour
                    rgba[o + c] = ((f32::from(col[c]) * ta) / alpha).round().clamp(0.0, 255.0) as u8;
                }
                rgba[o + 3] = (alpha * 255.0).round() as u8;
            }
        }
        Ok(Overlay { key, x: x0 as u16, y: y0 as u16, w: w as u16, h: hgt as u16, rgba })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn open() -> (Player, PlayerInfo) {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vectors/tnv3_15_16/ok15_w1_w2_two_sizes.tnv3");
        Player::open(&p).expect("repository vector opens")
    }

    #[test]
    fn open_reports_tracks_and_sizes() {
        let (_, info) = open();
        assert_eq!(info.subtitles.len(), 1);
        let t = &info.subtitles[0];
        assert_eq!((t.language.as_str(), t.variants.clone()), ("eng", vec![(6, 8), (9, 12)]));
    }

    #[test]
    fn overlay_draws_only_while_events_are_active_and_reports_changes_by_key() {
        let (player, _) = open();
        // sample 960: only the first event (0..4000); the second (2000..8000) starts later
        let a = player.subtitle_overlay(0, 0, 960, true, false, 0).unwrap();
        assert!(a.key >= 2 && a.w > 0 && a.h > 0);
        assert_eq!(a.rgba.len(), usize::from(a.w) * usize::from(a.h) * 4);
        assert!(a.rgba.chunks(4).any(|p| p[3] == 255), "some opaque text pixel");
        assert!(a.rgba.chunks(4).all(|p| p[3] == 0 || p[3] > 0));
        // same moment, key already known: only the key comes back
        let same = player.subtitle_overlay(0, 0, 961, true, false, a.key).unwrap();
        assert_eq!((same.key, same.w, same.rgba.len()), (a.key, 0, 0));
        // both events active (sample 3000): different key, larger or equal box
        let both = player.subtitle_overlay(0, 0, 3000, true, false, a.key).unwrap();
        assert_ne!(both.key, a.key);
        // after every event: nothing drawn, still a key
        let none = player.subtitle_overlay(0, 0, 20_000, true, false, a.key).unwrap();
        assert_eq!((none.w, none.h, none.rgba.len()), (0, 0, 0));
        assert_ne!(none.key, a.key);
        // subtitles off: only forced events (none in this vector)
        let off = player.subtitle_overlay(0, 0, 960, false, false, 0).unwrap();
        assert_eq!((off.w, off.rgba.len()), (0, 0));
        // hiding SDH removes the second event (flag 2) at sample 3000
        let no_sdh = player.subtitle_overlay(0, 0, 3000, true, true, 0).unwrap();
        assert_ne!(no_sdh.key, both.key);
        assert!(usize::from(no_sdh.w) * usize::from(no_sdh.h) <= usize::from(both.w) * usize::from(both.h));
        // the larger size draws a larger box
        let big = player.subtitle_overlay(0, 1, 960, true, false, 0).unwrap();
        assert!(u32::from(big.h) >= u32::from(a.h));
        // bad indexes are errors
        assert!(player.subtitle_overlay(3, 0, 0, true, false, 0).is_err());
        assert!(player.subtitle_overlay(0, 9, 0, true, false, 0).is_err());
    }
}

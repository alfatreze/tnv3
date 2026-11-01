use serde::Serialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use tau_video::{
    Tnv3Reader,
    tvc::frame::{Frame420, PictureDecoder},
};

#[derive(Serialize)]
pub struct PlayerInfo {
    pub width: u16,
    pub height: u16,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frames: u32,
    pub audio: Option<String>,
}
pub struct Player {
    reader: Tnv3Reader<File>,
    file: File,
    decoder: PictureDecoder,
    next: u32,
    finished: bool,
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
        let result = PlayerInfo {
            width: info.width,
            height: info.height,
            fps_num: info.fps_num,
            fps_den: info.fps_den,
            frames: info.frame_count,
            audio,
        };
        Ok((
            Self {
                reader,
                file: File::open(path).map_err(|e| e.to_string())?,
                decoder,
                next: 0,
                finished: false,
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

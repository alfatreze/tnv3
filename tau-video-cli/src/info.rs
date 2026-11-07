//! TNV3 `info` command formatting.
//!
//! This reports metadata verified by `Tnv3Reader::open`: the header (including
//! its CRC) and declared index/layout. It deliberately does not claim that
//! chunk payloads or the whole container have been checked.

use std::io::{Read, Seek};

use serde_json::{Value, json};
use tau_video::{Error, Info, Tnv3Reader};

/// Metadata and validation scope returned by the info command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoReport {
    /// Header/index metadata accepted by the reader.
    pub info: Info,
    /// TNV3 version verified by the reader.
    pub version: u16,
    /// Header CRC was validated by `Tnv3Reader::open`.
    pub header_crc_ok: bool,
    /// The declared index and file layout bounds were validated by the reader; entries were not read.
    pub index_layout_ok: bool,
    /// Chunk payloads were not inspected by this command.
    pub chunks_checked: bool,
}

/// Open a TNV3 stream and return only information validated by the reader.
pub fn inspect<R: Read + Seek>(input: R) -> Result<InfoReport, Error> {
    let reader = Tnv3Reader::open(input)?;
    Ok(InfoReport {
        info: reader.info().clone(),
        version: tau_video::TNV3_VERSION,
        header_crc_ok: true,
        index_layout_ok: true,
        chunks_checked: false,
    })
}

impl InfoReport {
    /// Stable schema-1 machine representation for `info FILE --json`.
    pub fn to_json(&self) -> Value {
        let i = &self.info;
        json!({
            "schema": 1,
            "command": "info",
            "validation": {
                "scope": "header_and_index_layout",
                "header_crc_ok": self.header_crc_ok,
                "index_layout_ok": self.index_layout_ok,
                "index_entries_checked": false,
                "chunks_checked": self.chunks_checked
            },
            "info": {
                "version": self.version,
                "width": i.width,
                "height": i.height,
                "fps_num": i.fps_num,
                "fps_den": i.fps_den,
                "level": i.level,
                "frame_count": i.frame_count,
                "audio_channels": i.audio_channels,
                "audio_samples": i.audio_samples,
                "tac_frames": i.tac_frames,
                "flags": i.flags,
                "index_offset": i.index_offset,
                "subtitle_offset": i.subtitle_offset,
                "subtitle_length": i.subtitle_length,
                "file_size": i.file_size
            }
        })
    }

    /// Human-readable report with an explicit validation limitation.
    pub fn to_human(&self) -> String {
        let i = &self.info;
        format!(
            "TNV3 v{} {}x{}, {}/{}, level {}, {} frames, {} audio channels, {} bytes\nHeader CRC and declared layout: valid. Index entries and chunk contents: not checked.",
            self.version,
            i.width,
            i.height,
            i.fps_num,
            i.fps_den,
            i.level,
            i.frame_count,
            i.audio_channels,
            i.file_size
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use tau_video::crc32;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 100];
        bytes[0..4].copy_from_slice(b"TNV3");
        bytes[8..10].copy_from_slice(&16u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&16u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&24u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20] = 1;
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&84u32.to_le_bytes());
        let crc = crc32(&bytes[..60]);
        bytes[60..64].copy_from_slice(&crc.to_le_bytes());
        bytes[84..88].copy_from_slice(&64u32.to_le_bytes());
        bytes
    }

    #[test]
    fn reports_verified_header_and_index_without_claiming_chunk_check() {
        let report = inspect(Cursor::new(fixture())).unwrap();
        assert_eq!((report.info.width, report.info.height), (16, 16));
        assert!(report.header_crc_ok);
        assert!(report.index_layout_ok);
        assert!(!report.chunks_checked);

        let json = report.to_json();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["validation"]["scope"], "header_and_index_layout");
        assert_eq!(json["validation"]["chunks_checked"], false);
        assert_eq!(json["info"]["frame_count"], 1);
    }

    #[test]
    fn human_output_states_validation_limit() {
        let report = inspect(Cursor::new(fixture())).unwrap();
        let output = report.to_human();
        assert!(output.contains("Header CRC and declared layout: valid."));
        assert!(output.contains("Index entries and chunk contents: not checked."));
    }

    #[test]
    fn malformed_header_is_returned_as_reader_error() {
        let mut bytes = fixture();
        bytes[60] ^= 1;
        assert!(inspect(Cursor::new(bytes)).is_err());
    }
}

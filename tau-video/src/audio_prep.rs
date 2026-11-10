//! Audio preparation shared by the app and the command line: a surround-to-stereo downmix with a stronger centre
//! (dialogue) plus loudness normalisation, and the `tac enc` options chosen by the encoder settings.
//!
//! Only sources with more than two channels are touched. Stereo and mono sources, or the option off, give no filter at
//! all, so their audio is processed exactly as before. Nothing here runs a program except [`analysis_command`] building
//! one; callers run it under their own supervision.
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

/// Downmix and loudness settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioPrep {
    /// Mix sources with more than two channels to stereo with the centre up, and normalise their loudness.
    pub downmix_enhance: bool,
    /// Loudness target in LUFS.
    pub loudness_lufs: f64,
}
impl Default for AudioPrep {
    fn default() -> Self {
        Self { downmix_enhance: true, loudness_lufs: -18.0 }
    }
}
impl AudioPrep {
    /// The same settings with the target forced into a sensible range (-40..-5 LUFS), else the default.
    pub fn checked_target(&self) -> f64 {
        if (-40.0..=-5.0).contains(&self.loudness_lufs) {
            self.loudness_lufs
        } else {
            -18.0
        }
    }
}

/// `tac enc` options chosen by the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TacTuning {
    /// `-L dB`: stop spending bits once the noise is this many dB under the masking threshold (-30..12); `None` = whole budget.
    pub floor_db: Option<f64>,
    /// `-D 0.28`: wider quantiser dead zone (encoder only; the stream format is unchanged).
    pub tuned: bool,
}
impl TacTuning {
    /// The arguments to add after `tac enc IN OUT -b KBPS`. An out-of-range floor is ignored.
    pub fn args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(f) = self.floor_db.filter(|f| (-30.0..=12.0).contains(f)) {
            a.extend(["-L".to_string(), f.to_string()]);
        }
        if self.tuned {
            a.extend(["-D".to_string(), "0.28".to_string()]);
        }
        a
    }
}

/// Result of the first loudness pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub input_i: String,
    pub input_tp: String,
    pub input_lra: String,
    pub input_thresh: String,
    pub target_offset: String,
}

/// 5.1 or 7.1 down to stereo: centre 3 dB up, surrounds 6 dB down, no bass channel, scaled so the sum cannot clip.
const DOWNMIX: &str = "aresample=ochl=stereo:center_mix_level=1:surround_mix_level=0.5:lfe_mix_level=0:rematrix_maxval=1";

/// Whether the settings and the source's channel count call for the downmix.
pub fn applies(prep: &AudioPrep, source_channels: Option<u32>) -> bool {
    prep.downmix_enhance && source_channels.is_some_and(|c| c > 2)
}

/// Reads the channel count from `ffprobe -show_entries stream=channels -of csv=p=0` output.
pub fn channels_from_probe(stdout: &str) -> Option<u32> {
    stdout.lines().next()?.trim().parse().ok()
}

/// The channel count of the chosen audio stream, or `None` when FFprobe cannot say.
pub fn source_channels(ffprobe: &Path, input: &Path, track: u32) -> Option<u32> {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", &format!("a:{track}"), "-show_entries", "stream=channels", "-of", "csv=p=0", "-i"])
        .arg(input)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    channels_from_probe(&String::from_utf8_lossy(&out.stdout))
}

/// The filter of the first pass: downmix, then `loudnorm` printing its measurements as JSON.
pub fn analysis_filter(prep: &AudioPrep) -> String {
    format!("{DOWNMIX},loudnorm=I={}:TP=-1.5:LRA=15:print_format=json", prep.checked_target())
}

/// FFFmpeg's report file option must not contain `:` or `\` unescaped.
fn ffreport_escape(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\").replace(':', "\\:").replace('\'', "\\'")
}

/// The first-pass command. FFmpeg's log (where `loudnorm` prints) goes to `report` through `FFREPORT`, so the caller can
/// run it with whatever supervision it has and read the whole log afterwards with [`read_analysis`].
pub fn analysis_command(ffmpeg: &Path, input: &Path, track: u32, prep: &AudioPrep, report: &Path) -> Command {
    let mut c = Command::new(ffmpeg);
    c.env("FFREPORT", OsString::from(format!("file={}:level=32", ffreport_escape(report))))
        .args(["-nostdin", "-hide_banner", "-nostats", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-map", &format!("0:a:{track}"), "-vn", "-af"])
        .arg(analysis_filter(prep))
        .args(["-f", "null", "-"]);
    c
}

/// Reads the measurements from the report written by [`analysis_command`] (then removes it).
pub fn read_analysis(report: &Path) -> Option<Measured> {
    let log = std::fs::read_to_string(report).ok();
    let _ = std::fs::remove_file(report);
    parse_loudnorm(&log?)
}

/// Reads the JSON block `loudnorm` prints after its first pass; `None` for a missing block or non-finite values
/// (silent audio).
pub fn parse_loudnorm(log: &str) -> Option<Measured> {
    let start = log.rfind('{')?;
    let end = log.rfind('}')?;
    let block = log.get(start..=end)?;
    // the block is flat: "key" : "value" pairs (values are quoted numbers)
    let field = |k: &str| -> Option<String> {
        let rest = block.split_once(&format!("\"{k}\""))?.1;
        let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
        let t = rest.split_once('"')?.0.trim().to_owned();
        t.parse::<f64>().ok().filter(|x| x.is_finite())?;
        Some(t)
    };
    Some(Measured {
        input_i: field("input_i")?,
        input_tp: field("input_tp")?,
        input_lra: field("input_lra")?,
        input_thresh: field("input_thresh")?,
        target_offset: field("target_offset")?,
    })
}

/// The filter chain for the real pass (before any padding): the downmix, plus a linear (or, where the peak ceiling
/// forbids that, dynamic) `loudnorm` using the measurements, or the downmix alone when the analysis failed.
pub fn filter_chain(prep: &AudioPrep, measured: Option<&Measured>) -> String {
    match measured {
        Some(m) => format!(
            "{DOWNMIX},loudnorm=I={}:TP=-1.5:LRA=15:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
            prep.checked_target(),
            m.input_i,
            m.input_tp,
            m.input_lra,
            m.input_thresh,
            m.target_offset
        ),
        None => DOWNMIX.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applies_only_to_surround_sources_with_the_option_on() {
        let on = AudioPrep::default();
        assert!(applies(&on, Some(6)) && applies(&on, Some(8)) && applies(&on, Some(3)));
        assert!(!applies(&on, Some(2)) && !applies(&on, Some(1)) && !applies(&on, None));
        assert!(!applies(&AudioPrep { downmix_enhance: false, ..on }, Some(6)));
    }
    #[test]
    fn probe_output_and_targets_are_read_strictly() {
        assert_eq!(channels_from_probe("6\n"), Some(6));
        assert_eq!(channels_from_probe("N/A\n"), None);
        assert_eq!(AudioPrep { loudness_lufs: 5.0, ..Default::default() }.checked_target(), -18.0);
        assert_eq!(AudioPrep { loudness_lufs: -23.0, ..Default::default() }.checked_target(), -23.0);
    }
    #[test]
    fn loudnorm_report_is_parsed_and_bad_reports_are_refused() {
        let log = "noise {not json}\n[Parsed_loudnorm_0 @ 0x1]\n{\n\t\"input_i\" : \"-31.42\",\n\t\"input_tp\" : \"-10.89\",\n\t\"input_lra\" : \"12.00\",\n\t\"input_thresh\" : \"-42.29\",\n\t\"target_offset\" : \"0.58\"\n}\n";
        let m = parse_loudnorm(log).unwrap();
        assert_eq!((m.input_i.as_str(), m.input_tp.as_str(), m.target_offset.as_str()), ("-31.42", "-10.89", "0.58"));
        let chain = filter_chain(&AudioPrep::default(), Some(&m));
        assert!(chain.contains("measured_I=-31.42") && chain.contains("offset=0.58") && chain.ends_with("linear=true"), "{chain}");
        assert_eq!(filter_chain(&AudioPrep::default(), None), DOWNMIX, "failed analysis falls back to the downmix alone");
        assert!(parse_loudnorm("{\"input_i\":\"-inf\",\"input_tp\":\"-inf\",\"input_lra\":\"0\",\"input_thresh\":\"-70\",\"target_offset\":\"0\"}").is_none());
        assert!(parse_loudnorm("no report here").is_none());
    }
    #[test]
    fn tac_options_are_validated() {
        assert!(TacTuning::default().args().is_empty());
        assert_eq!(TacTuning { floor_db: Some(0.0), tuned: true }.args(), ["-L", "0", "-D", "0.28"]);
        assert!(TacTuning { floor_db: Some(99.0), tuned: false }.args().is_empty());
    }
    #[test]
    fn report_path_is_escaped_for_ffreport() {
        assert_eq!(ffreport_escape(Path::new("/a b/c:d")), "/a b/c\\:d");
    }
}

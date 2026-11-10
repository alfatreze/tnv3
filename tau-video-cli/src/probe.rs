//! Bounded FFprobe metadata parser and standalone process adapter.
//!
//! The CLI adapter gathers selected FFprobe metadata and may decode one frame
//! through the reference scaling filter. An explicitly requested sample also
//! measures a short TVC video-only segment; neither mode validates the full
//! media stream or estimates complete output size/time.

use serde_json::Value;
use std::path::Path;

pub const MAX_PROBE_JSON_BYTES: usize = 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub const MAX_PROBE_STDERR_BYTES: usize = 16 * 1024;
pub const DEFAULT_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Output geometry profile of the estimate: pictures at most `width` wide and `max_height()` tall, even sizes,
/// 24 fps. The default is the 400-wide reference profile; `--width` selects another (height is 9/10 of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub width: u32,
}
impl Default for Profile {
    fn default() -> Self {
        Profile { width: 400 }
    }
}
impl Profile {
    pub fn max_height(self) -> u32 {
        (self.width * 9 / 10) & !1
    }
    pub fn name(self) -> String {
        format!("pocket-default-reference-{}x{}-24fps", self.width, self.max_height())
    }
    pub fn filter(self) -> String {
        format!(
            "scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24",
            self.width,
            self.max_height()
        )
    }
}

pub const ESTIMATE_SAMPLE_FRAMES: u32 = 48;

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeInfo {
    pub format_name: Option<String>,
    pub duration_seconds: Option<f64>,
    pub bit_rate: Option<u64>,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub other_streams: usize,
    /// Geometry from one decoded frame through the explicit default Pocket profile.
    /// Output profile the geometry and estimates refer to.
    pub profile: Profile,
    pub output_geometry: Option<(u32, u32)>,
    pub output_level: Option<u8>,
    pub output_geometry_error: Option<String>,
    /// Whole-output totals remain unavailable; sample-only estimates are separate.
    pub estimated_output_bytes: Option<u64>,
    pub estimated_encode_seconds: Option<f64>,
    pub estimated_video_payload_bytes: Option<u64>,
    pub estimated_video_stage_seconds: Option<f64>,
    pub estimate_sample_frames: Option<u32>,
    pub estimate_sample_level: Option<u8>,
    pub estimate_sample_output_bytes: Option<u64>,
    pub estimate_sample_media_seconds: Option<f64>,
    pub estimate_measured_wall_seconds: Option<f64>,
    pub estimate_duration_basis: Option<&'static str>,
    pub estimate_status: &'static str,
}

impl ProbeInfo {
    /// Preview JSON shape for `probe --json`. Whole-output totals remain null;
    /// an opt-in TVC sample adds scoped video-only projections.
    pub fn to_json_value(&self) -> Value {
        serde_json::json!({
            "schema": 1,
            "event": "probe",
            "command": "probe",
            "scope": if self.estimate_status == "sampled_video_only" {"source-metadata-default-profile-first-frame-and-sample"} else {"source-metadata-and-default-profile-first-frame"},
            "packets_checked": false,
            "first_frame_decoded": self.output_geometry.is_some(),
            "output_profile": {"name": self.profile.name(), "filter": self.profile.filter(),
                "scope": "reference filter preview; custom scripts/settings may differ",
                "selected_video_stream_index": self.video.first().and_then(|v| v.index),
                "ffmpeg_autorotation": "enabled by FFmpeg default",
                "sar_handling": "coded raster dimensions; scaling may preserve or adjust SAR rather than baking it into square pixels; output SAR is not reported"},
            "output_geometry": self.output_geometry.map(|(width, height)| serde_json::json!({"width": width, "height": height})),
            "output_geometry_error": self.output_geometry_error.as_deref(),
            "output_level": self.output_level.map(|minimum_fitting| serde_json::json!({
                "minimum_fitting": minimum_fitting,
                "basis": "output geometry and frame rate only; final encoding may select a higher level for picture-size limits"
            })),
            "format": self.format_name,
            "duration_seconds": self.duration_seconds,
            "bit_rate": self.bit_rate,
            "video": self.video.iter().map(|v| serde_json::json!({
                "index": v.index, "codec": v.codec, "width": v.width,
                "height": v.height, "frame_rate": v.frame_rate,
                "pixel_format": v.pixel_format,
                "sample_aspect_ratio": v.sample_aspect_ratio,
                "display_aspect_ratio": v.display_aspect_ratio,
                "rotation_degrees": v.rotation_degrees,
                "duration_seconds": v.duration_seconds
            })).collect::<Vec<_>>(),
            "audio": self.audio.iter().map(|a| serde_json::json!({
                "index": a.index, "codec": a.codec, "sample_rate": a.sample_rate,
                "channels": a.channels, "bit_rate": a.bit_rate,
                "language": a.language, "title": a.title
            })).collect::<Vec<_>>(),
            "other_streams": self.other_streams,
            "estimate": {
                "output_bytes": self.estimated_output_bytes,
                "encode_seconds": self.estimated_encode_seconds,
                "video_payload_bytes": self.estimated_video_payload_bytes,
                "video_stage_seconds": self.estimated_video_stage_seconds,
                "status": self.estimate_status,
                "sample_settings": {"profile": self.profile.name(), "qp": 30, "level": self.estimate_sample_level},
                "scope": "video-only TVC sample extrapolation; excludes TNV3 container/index, MP3 sidecar encoding and mux; measured wall time includes sample decode/encode and process startup",
                "sample_frames": self.estimate_sample_frames,
                "sample_output_bytes": self.estimate_sample_output_bytes,
                "sample_media_seconds": self.estimate_sample_media_seconds,
                "measured_wall_seconds": self.estimate_measured_wall_seconds,
                "projection_duration_basis": self.estimate_duration_basis,
                "uncertainty": "short content sample may not represent later scene complexity; startup overhead is scaled with the sample; projected values are not a bound"
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoStream {
    pub index: Option<u64>,
    pub codec: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub frame_rate: Option<String>,
    pub duration_seconds: Option<f64>,
    pub pixel_format: Option<String>,
    /// FFprobe SAR belongs to the coded raster; width/height remain coded dimensions.
    pub sample_aspect_ratio: Option<String>,
    pub display_aspect_ratio: Option<String>,
    pub rotation_degrees: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioStream {
    pub index: Option<u64>,
    pub codec: Option<String>,
    pub sample_rate: Option<u64>,
    pub channels: Option<u64>,
    pub bit_rate: Option<u64>,
    /// `language` tag (for example "eng"), if the file has one.
    pub language: Option<String>,
    /// `title` tag, if the file has one.
    pub title: Option<String>,
}

#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    TooLarge,
    InvalidJson,
    InvalidShape,
    InvalidShapeWithContext(String),
    Spawn(String),
    GroupSetupFailed(String),
    CleanupUnconfirmed(String),
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
    TimedOut,
    Wait(String),
    OwnershipLost(String),
    GroupSignalFailed(String),
    KillUnconfirmed,
    DescendantPipeLeak,
    OutputTooLarge {
        stream: &'static str,
        limit: usize,
    },
    ToolFailed {
        status: Option<i32>,
        stderr: String,
        stderr_truncated: bool,
    },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("ffprobe JSON exceeds the 1 MiB limit"),
            Self::InvalidJson => f.write_str("ffprobe returned invalid JSON"),
            Self::InvalidShape => f.write_str("ffprobe JSON has an invalid top-level shape"),
            Self::InvalidShapeWithContext(e) => {
                write!(f, "invalid FFmpeg Y4M geometry output: {e}")
            }
            Self::Spawn(e) => write!(f, "could not start media probe tool: {e}"),
            Self::GroupSetupFailed(e) => write!(f, "media probe process-group setup failed: {e}"),
            Self::CleanupUnconfirmed(e) => write!(f, "media probe cleanup is unconfirmed: {e}"),
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            Self::UnsupportedPlatform => f.write_str(
                "probe execution requires POSIX process-group and nonblocking-pipe support",
            ),
            Self::TimedOut => f.write_str("media probe timed out"),
            Self::Wait(e) => write!(f, "could not wait for media probe tool: {e}"),
            Self::OwnershipLost(e) => write!(
                f,
                "lost wait ownership of media probe; refusing process-group signal: {e}"
            ),
            Self::GroupSignalFailed(e) => write!(
                f,
                "could not signal media probe process group; cleanup not claimed: {e}"
            ),
            Self::KillUnconfirmed => {
                f.write_str("could not confirm POSIX process-group termination")
            }
            Self::DescendantPipeLeak => f.write_str(
                "a process-group descendant kept an output pipe open after the media tool exited",
            ),
            Self::OutputTooLarge { stream, limit } => {
                write!(f, "media tool {stream} exceeded the {limit}-byte limit")
            }
            Self::ToolFailed {
                status,
                stderr,
                stderr_truncated,
            } => {
                write!(f, "media tool failed (status {status:?}): {stderr}")?;
                if *stderr_truncated {
                    f.write_str(" [stderr truncated]")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// Execute ffprobe using a POSIX process group and bounded nonblocking pipes.
pub fn run_probe(ffprobe: &Path, input: &Path) -> Result<ProbeInfo, ProbeError> {
    run_probe_with_timeout(ffprobe, input, DEFAULT_PROBE_TIMEOUT)
}

pub fn run_probe_with_timeout(
    ffprobe: &Path,
    input: &Path,
    timeout: std::time::Duration,
) -> Result<ProbeInfo, ProbeError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        crate::probe_exec::run_probe_with_timeout(ffprobe, input, timeout)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (ffprobe, input, timeout);
        Err(ProbeError::UnsupportedPlatform)
    }
}

/// Run metadata and the actual default Pocket FFmpeg filter. Geometry is read
/// from FFmpeg's Y4M header, so scale rounding and autorotation are FFmpeg's.
pub fn run_probe_with_tools(
    ffprobe: &Path,
    ffmpeg: Option<&Path>,
    input: &Path,
    profile: Profile,
) -> Result<ProbeInfo, ProbeError> {
    let mut report = run_probe(ffprobe, input)?;
    report.profile = profile;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let geometry_result = match ffmpeg {
            Some(ffmpeg) => crate::probe_exec::run_geometry(ffmpeg, input, DEFAULT_PROBE_TIMEOUT, profile)
                .and_then(|bytes| parse_y4m_output(&bytes, profile)),
            None => Err(ProbeError::Spawn("FFmpeg not found; use --ffmpeg".into())),
        };
        apply_geometry_result(&mut report, geometry_result)?;
        Ok(report)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (ffmpeg, input);
        report.output_geometry_error = Some(ProbeError::UnsupportedPlatform.to_string());
        Ok(report)
    }
}

/// Measure a short default-profile TVC video sample and extrapolate only video
/// payload size and decode-plus-encode wall time. This is opt-in and requires
/// the same local FFmpeg/TVC tools used by the selected workflow.
pub fn apply_sample_estimate(
    report: &mut ProbeInfo,
    input: &Path,
    ffmpeg: &Path,
    tvc_encoder: &Path,
) -> Result<(), ProbeError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if report.video.is_empty() {
            return Err(ProbeError::InvalidShapeWithContext(
                "sample estimate requires a video stream".into(),
            ));
        }
        let started = std::time::Instant::now();
        let y4m = crate::probe_exec::run_sample_frames(
            ffmpeg,
            input,
            ESTIMATE_SAMPLE_FRAMES,
            std::time::Duration::from_secs(30),
            report.profile,
        )?;
        let (geometry, level, frame_count) = parse_y4m_sample(&y4m, report.profile)?;
        if report
            .output_geometry
            .is_some_and(|expected| expected != geometry)
        {
            return Err(ProbeError::InvalidShapeWithContext(
                "sample geometry differs from the one-frame geometry probe".into(),
            ));
        }
        let sample_bytes = crate::probe_exec::encode_tvc_sample(
            tvc_encoder,
            &y4m,
            level,
            std::time::Duration::from_secs(120),
        )?;
        let measured_wall_seconds = started.elapsed().as_secs_f64();
        let sample_media_seconds = f64::from(frame_count) / 24.0;
        report.estimate_sample_frames = Some(frame_count);
        report.estimate_sample_level = Some(level);
        report.estimate_sample_output_bytes = Some(sample_bytes);
        report.estimate_sample_media_seconds = Some(sample_media_seconds);
        report.estimate_measured_wall_seconds = Some(measured_wall_seconds);
        report.estimate_status = "sampled_video_only";
        let (duration, basis) = report
            .video
            .first()
            .and_then(|stream| {
                stream
                    .duration_seconds
                    .map(|v| (v, "first_video_stream_duration"))
            })
            .or_else(|| {
                report
                    .duration_seconds
                    .map(|v| (v, "container_format_duration_fallback"))
            })
            .unwrap_or((f64::NAN, "unavailable"));
        report.estimate_duration_basis = Some(basis);
        if duration.is_finite() && duration > 0.0 {
            let total_frames = (duration * 24.0).ceil();
            let scale = total_frames / f64::from(frame_count);
            let bytes = (sample_bytes as f64 * scale).ceil();
            if bytes.is_finite() && bytes <= u64::MAX as f64 {
                report.estimated_video_payload_bytes = Some(bytes as u64);
                let projected_seconds = measured_wall_seconds * scale;
                if projected_seconds.is_finite() {
                    report.estimated_video_stage_seconds = Some(projected_seconds);
                }
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (report, ffmpeg, tvc_encoder);
        Err(ProbeError::UnsupportedPlatform)
    }
}

fn parse_y4m_sample(bytes: &[u8], profile: Profile) -> Result<((u32, u32), u8, u32), ProbeError> {
    use std::io::{BufReader, Cursor};
    let mut groups = tau_video::y4m::Y4mFrameGroups::new(BufReader::new(Cursor::new(bytes)), 1)
        .map_err(|e| ProbeError::InvalidShapeWithContext(e.to_string()))?;
    let header = groups.header().to_vec();
    let mut count = 0u32;
    let mut sink = Vec::new();
    while groups
        .write_next_group(&mut sink)
        .map_err(|e| ProbeError::InvalidShapeWithContext(e.to_string()))?
        .is_some()
    {
        count = count.checked_add(1).ok_or(ProbeError::InvalidShape)?;
        sink.clear();
    }
    if count == 0 || count > ESTIMATE_SAMPLE_FRAMES {
        return Err(ProbeError::InvalidShapeWithContext(
            "sample frame count is outside the requested bound".into(),
        ));
    }
    let (geometry, _) = parse_y4m_output(bytes, profile)?;
    let level = (1..=3)
        .find(|&candidate| {
            tau_video::c_orchestrator::check_level_fit(&header, candidate).is_ok()
        })
        .ok_or_else(|| {
            ProbeError::InvalidShapeWithContext("sample geometry does not fit a TVC level".into())
        })?;
    Ok((geometry, level, count))
}

fn apply_geometry_result(
    report: &mut ProbeInfo,
    result: Result<((u32, u32), u8), ProbeError>,
) -> Result<(), ProbeError> {
    match result {
        Ok((geometry, level)) => {
            report.output_geometry = Some(geometry);
            report.output_level = Some(level);
        }
        Err(error) if geometry_cleanup_uncertain(&error) => return Err(error),
        Err(error) => report.output_geometry_error = Some(error.to_string()),
    }
    Ok(())
}

fn geometry_cleanup_uncertain(error: &ProbeError) -> bool {
    matches!(
        error,
        ProbeError::CleanupUnconfirmed(_)
            | ProbeError::OwnershipLost(_)
            | ProbeError::GroupSignalFailed(_)
            | ProbeError::KillUnconfirmed
            | ProbeError::DescendantPipeLeak
            | ProbeError::Wait(_)
            | ProbeError::GroupSetupFailed(_)
    )
}

fn parse_y4m_output(bytes: &[u8], profile: Profile) -> Result<((u32, u32), u8), ProbeError> {
    use std::io::{BufReader, Cursor};
    let mut reader = tau_video::y4m::Y4mFrameGroups::new(BufReader::new(Cursor::new(bytes)), 1)
        .map_err(|e| ProbeError::InvalidShapeWithContext(e.to_string()))?;
    let header = reader.header().to_vec();
    let mut one_frame = Vec::new();
    reader
        .write_next_group(&mut one_frame)
        .map_err(|e| ProbeError::InvalidShapeWithContext(e.to_string()))?
        .ok_or_else(|| ProbeError::InvalidShapeWithContext("FFmpeg returned no frame".into()))?;
    let text = std::str::from_utf8(&header).map_err(|_| ProbeError::InvalidShape)?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    for field in text.split_ascii_whitespace().skip(1) {
        if let Some(v) = field.strip_prefix('W') {
            width = v.parse::<u32>().ok();
        }
        if let Some(v) = field.strip_prefix('H') {
            height = v.parse::<u32>().ok();
        }
        if let Some(v) = field.strip_prefix('F') {
            if fps.replace(v).is_some() {
                return Err(ProbeError::InvalidShapeWithContext(
                    "duplicate frame rate".into(),
                ));
            }
        }
    }
    let geometry = width.zip(height).ok_or(ProbeError::InvalidShape)?;
    if geometry.0 > profile.width
        || geometry.1 > profile.max_height()
        || geometry.0 % 2 != 0
        || geometry.1 % 2 != 0
        || fps != Some("24:1")
    {
        return Err(ProbeError::InvalidShapeWithContext(
            format!(
                "output does not match the {}x{} even 24 fps reference profile",
                profile.width,
                profile.max_height()
            ),
        ));
    }
    let level = (1..=3)
        .find(|&l| tau_video::c_orchestrator::check_level_fit(&header, l).is_ok())
        .ok_or_else(|| {
            ProbeError::InvalidShapeWithContext("Y4M output does not fit TVC levels 1-3".into())
        })?;
    Ok((geometry, level))
}

/// Parse the stable subset requested by `-show_format -show_streams -of json`.
/// Unknown/missing optional fields remain `None`; unknown stream types are
/// counted, not silently represented as audio/video.
pub fn parse_ffprobe_json(bytes: &[u8]) -> Result<ProbeInfo, ProbeError> {
    if bytes.len() > MAX_PROBE_JSON_BYTES {
        return Err(ProbeError::TooLarge);
    }
    let root: Value = serde_json::from_slice(bytes).map_err(|_| ProbeError::InvalidJson)?;
    let obj = root.as_object().ok_or(ProbeError::InvalidShape)?;
    let streams = obj
        .get("streams")
        .and_then(Value::as_array)
        .ok_or(ProbeError::InvalidShape)?;
    let format = obj.get("format").and_then(Value::as_object);
    let mut report = ProbeInfo {
        format_name: format.and_then(|f| string(f.get("format_name"))),
        duration_seconds: format.and_then(|f| numeric(f.get("duration"))),
        bit_rate: format.and_then(|f| unsigned(f.get("bit_rate"))),
        video: Vec::new(),
        audio: Vec::new(),
        other_streams: 0,
        profile: Profile::default(),
        output_geometry: None,
        output_level: None,
        output_geometry_error: None,
        estimated_output_bytes: None,
        estimated_encode_seconds: None,
        estimated_video_payload_bytes: None,
        estimated_video_stage_seconds: None,
        estimate_sample_frames: None,
        estimate_sample_level: None,
        estimate_sample_output_bytes: None,
        estimate_sample_media_seconds: None,
        estimate_measured_wall_seconds: None,
        estimate_duration_basis: None,
        estimate_status: "unavailable",
    };
    for stream in streams {
        let Some(s) = stream.as_object() else {
            return Err(ProbeError::InvalidShape);
        };
        let kind = s.get("codec_type").and_then(Value::as_str);
        match kind {
            Some("video") => report.video.push(VideoStream {
                index: unsigned(s.get("index")),
                codec: string(s.get("codec_name")),
                width: unsigned(s.get("width")),
                height: unsigned(s.get("height")),
                frame_rate: string(s.get("avg_frame_rate"))
                    .or_else(|| string(s.get("r_frame_rate"))),
                duration_seconds: numeric(s.get("duration")),
                pixel_format: string(s.get("pix_fmt")),
                sample_aspect_ratio: string(s.get("sample_aspect_ratio")),
                display_aspect_ratio: string(s.get("display_aspect_ratio")),
                rotation_degrees: s.get("side_data_list").and_then(Value::as_array).and_then(
                    |items| {
                        items
                            .iter()
                            .find_map(|item| signed_numeric(item.get("rotation")))
                    },
                ),
            }),
            Some("audio") => report.audio.push(AudioStream {
                index: unsigned(s.get("index")),
                codec: string(s.get("codec_name")),
                sample_rate: unsigned(s.get("sample_rate")),
                channels: unsigned(s.get("channels")),
                bit_rate: unsigned(s.get("bit_rate")),
                language: s.get("tags").and_then(|t| string(t.get("language"))),
                title: s.get("tags").and_then(|t| string(t.get("title"))),
            }),
            _ => report.other_streams += 1,
        }
    }
    Ok(report)
}

fn string(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn unsigned(v: Option<&Value>) -> Option<u64> {
    v.and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_u64())
    })
}

fn numeric(v: Option<&Value>) -> Option<f64> {
    let n = v.and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_f64())
    })?;
    (n.is_finite() && n >= 0.0).then_some(n)
}

fn signed_numeric(v: Option<&Value>) -> Option<f64> {
    let n = v.and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_f64())
    })?;
    n.is_finite().then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_follows_the_width_rule_and_bounds_geometry() {
        let d = Profile::default();
        assert_eq!((d.width, d.max_height()), (400, 360));
        assert_eq!(d.name(), "pocket-default-reference-400x360-24fps");
        assert_eq!(d.filter(), "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24");
        let w = Profile { width: 800 };
        assert_eq!((w.max_height(), w.name().as_str()), (720, "pocket-default-reference-800x720-24fps"));
        assert_eq!(Profile { width: 240 }.max_height(), 216);
        let y4m = |w: u32, h: u32| {
            let mut out = format!("YUV4MPEG2 W{w} H{h} F24:1 Ip A1:1 C420jpeg\nFRAME\n").into_bytes();
            out.resize(out.len() + (w * h * 3 / 2) as usize, 0);
            out
        };
        assert!(parse_y4m_output(&y4m(800, 450), d).is_err(), "800 wide does not fit the 400 profile");
        assert_eq!(parse_y4m_output(&y4m(800, 450), w).unwrap().0, (800, 450));
        assert!(parse_y4m_output(&y4m(800, 722), w).is_err());
    }

    #[test]
    fn production_y4m_and_level_validators_reject_bad_geometry_headers_and_frames() {
        let valid = |header: &str, marker: &str, bytes: usize| {
            let mut out = header.as_bytes().to_vec();
            out.extend_from_slice(marker.as_bytes());
            out.resize(out.len() + bytes, 0);
            out
        };
        let good = valid("YUV4MPEG2 W16 H16 F24:1 C420jpeg\n", "FRAME\n", 384);
        assert_eq!(parse_y4m_output(&good, Profile::default()).unwrap(), ((16, 16), 1));
        for bad in [
            valid("YUV4MPEG2 W16 W16 H16 F24:1 C420jpeg\n", "FRAME\n", 384),
            valid("YUV4MPEG2 W16 H16 F24:1 C444\n", "FRAME\n", 384),
            valid("YUV4MPEG2 W16 H16 F24:1 F24:1 C420jpeg\n", "FRAME\n", 384),
            valid("YUV4MPEG2 W16 H16 F25:1 C420jpeg\n", "FRAME\n", 384),
            valid("YUV4MPEG2 W15 H16 F24:1 C420jpeg\n", "FRAME\n", 368),
            valid("YUV4MPEG2 W16 H16 F24:1 C420jpeg\n", "FRAME-junk\n", 384),
            b"YUV4MPEG2 W16 H16 F24:1 C420jpeg\n".to_vec(),
            valid("YUV4MPEG2 W10000 H10000 F24:1 C420jpeg\n", "FRAME\n", 0),
        ] {
            assert!(parse_y4m_output(&bad, Profile::default()).is_err());
        }
    }

    #[test]
    fn uncertain_cleanup_is_never_downgraded_to_partial_success() {
        for error in [
            ProbeError::CleanupUnconfirmed("test".into()),
            ProbeError::OwnershipLost("test".into()),
            ProbeError::GroupSignalFailed("test".into()),
            ProbeError::KillUnconfirmed,
            ProbeError::DescendantPipeLeak,
            ProbeError::Wait("test".into()),
            ProbeError::GroupSetupFailed("test".into()),
        ] {
            assert!(geometry_cleanup_uncertain(&error));
        }
        assert!(!geometry_cleanup_uncertain(&ProbeError::Spawn(
            "missing".into()
        )));
        let mut report = parse_ffprobe_json(br#"{"streams":[],"format":{}}"#).unwrap();
        assert!(matches!(
            apply_geometry_result(&mut report, Err(ProbeError::KillUnconfirmed)),
            Err(ProbeError::KillUnconfirmed)
        ));
        apply_geometry_result(&mut report, Err(ProbeError::Spawn("missing".into()))).unwrap();
        assert!(report.output_geometry_error.is_some());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn geometry_matches_encoder_filter_for_odd_sar_and_rotated_inputs() {
        use std::{fs, process::Command};
        let ffmpeg = Path::new("ffmpeg");
        if Command::new(ffmpeg).arg("-version").output().is_err() {
            return;
        }
        let dir = crate::unique_test_dir("tau-probe-geometry");
        let odd = dir.join("odd-sar.mkv");
        let base = dir.join("rotation-base.mp4");
        let rotated = dir.join("rotated.mp4");
        let status = Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:size=641x359:rate=24:duration=1,format=yuv444p",
                "-vf",
                "setsar=3/2",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ])
            .arg(&odd)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=blue:size=640x360:rate=24:duration=1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&base)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new(ffmpeg)
            .args(["-v", "error", "-display_rotation:v:0", "90", "-i"])
            .arg(&base)
            .args(["-c", "copy"])
            .arg(&rotated)
            .status()
            .unwrap();
        assert!(status.success());

        let reference = |source: &Path| {
            let output = Command::new(ffmpeg)
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(source)
                .args([
                    "-map",
                    "0:v:0",
                    "-an",
                    "-sn",
                    "-vf",
                    Profile::default().filter().as_str(),
                    "-frames:v",
                    "1",
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "yuv4mpegpipe",
                    "pipe:1",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            parse_y4m_output(&output.stdout, Profile::default()).unwrap().0
        };
        let odd_result =
            crate::probe_exec::run_geometry(ffmpeg, &odd, DEFAULT_PROBE_TIMEOUT, Profile::default()).unwrap();
        let rotated_result =
            crate::probe_exec::run_geometry(ffmpeg, &rotated, DEFAULT_PROBE_TIMEOUT, Profile::default()).unwrap();
        let base_result =
            crate::probe_exec::run_geometry(ffmpeg, &base, DEFAULT_PROBE_TIMEOUT, Profile::default()).unwrap();
        let odd_geometry = parse_y4m_output(&odd_result, Profile::default()).unwrap().0;
        let rotated_geometry = parse_y4m_output(&rotated_result, Profile::default()).unwrap().0;
        let base_geometry = parse_y4m_output(&base_result, Profile::default()).unwrap().0;
        assert_eq!(odd_geometry, reference(&odd));
        assert_eq!(rotated_geometry, reference(&rotated));
        assert_eq!(base_geometry, reference(&base));
        assert_ne!(
            rotated_geometry, base_geometry,
            "FFmpeg autorotation should affect the aspect-fit output"
        );
        let odd_report = run_probe_with_tools(Path::new("ffprobe"), Some(ffmpeg), &odd, Profile::default()).unwrap();
        assert_eq!(
            odd_report.video[0].sample_aspect_ratio.as_deref(),
            Some("3:2")
        );
        let rotated_report =
            run_probe_with_tools(Path::new("ffprobe"), Some(ffmpeg), &rotated, Profile::default()).unwrap();
        assert_eq!(rotated_report.video[0].rotation_degrees, Some(90.0));
        assert!(
            odd_geometry.0 <= 400
                && odd_geometry.1 <= 360
                && odd_geometry.0 % 2 == 0
                && odd_geometry.1 % 2 == 0
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use std::time::{Duration, Instant};

    #[test]
    fn parses_video_audio_and_unknown_streams_without_inventing_values() {
        let json = br#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"24000/1001","pix_fmt":"yuv420p"},{"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2},{"index":2,"codec_type":"subtitle"}],"format":{"format_name":"mov,mp4","duration":"1.25","bit_rate":"900000"}}"#;
        let got = parse_ffprobe_json(json).unwrap();
        assert_eq!(got.video[0].width, Some(1280));
        assert_eq!(got.video[0].frame_rate.as_deref(), Some("24000/1001"));
        assert_eq!(got.audio[0].sample_rate, Some(48000));
        assert_eq!(got.audio[0].bit_rate, None);
        assert_eq!(got.other_streams, 1);
        assert_eq!(got.duration_seconds, Some(1.25));
        let json = got.to_json_value();
        assert_eq!(json["estimate"]["output_bytes"], Value::Null);
        assert_eq!(json["estimate"]["encode_seconds"], Value::Null);
        assert_eq!(json["estimate"]["status"], "unavailable");
    }

    #[test]
    fn missing_streams_and_bad_shapes_fail_closed() {
        assert_eq!(
            parse_ffprobe_json(br#"{"format":{}}"#),
            Err(ProbeError::InvalidShape)
        );
        assert_eq!(parse_ffprobe_json(br#"[]"#), Err(ProbeError::InvalidShape));
        assert_eq!(
            parse_ffprobe_json(b"not json"),
            Err(ProbeError::InvalidJson)
        );
    }

    #[test]
    fn caps_input_and_rejects_nonfinite_or_negative_duration() {
        assert_eq!(
            parse_ffprobe_json(&vec![b' '; MAX_PROBE_JSON_BYTES + 1]),
            Err(ProbeError::TooLarge)
        );
        let got = parse_ffprobe_json(br#"{"streams":[],"format":{"duration":"NaN"}}"#).unwrap();
        assert_eq!(got.duration_seconds, None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn command_captures_stderr_while_draining_both_pipes() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = std::env::temp_dir().join(format!("tau-probe-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let script = dir.join("ffprobe");
        fs::write(&script, "#!/bin/sh\nhead -c 8192 /dev/zero >&2\nprintf '{\\\"streams\\\":[],\\\"format\\\":{\\\"duration\\\":\\\"1.5\\\"}}'\n").unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        let report =
            run_probe_with_timeout(&script, Path::new("input file.mp4"), Duration::from_secs(2))
                .unwrap();
        assert_eq!(report.duration_seconds, Some(1.5));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn command_times_out_without_pipe_capture_or_blocking_wait() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = std::env::temp_dir().join(format!("tau-probe-timeout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let script = dir.join("ffprobe");
        fs::write(
            &script,
            "#!/bin/sh\nhead -c 262144 /dev/zero\nhead -c 8192 /dev/zero >&2\nsleep 5\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        let start = Instant::now();
        assert_eq!(
            run_probe_with_timeout(&script, Path::new("input"), Duration::from_millis(100)),
            Err(ProbeError::TimedOut)
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn kills_runaway_writers_on_both_streams_under_bounded_memory() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir =
            std::env::temp_dir().join(format!("tau-probe-both-streams-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let script = dir.join("ffprobe");
        fs::write(
            &script,
            "#!/bin/sh\nhead -c 4000000 /dev/zero & head -c 4000000 /dev/zero >&2 & wait\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        assert!(matches!(
            run_probe(&script, Path::new("unused")),
            Err(ProbeError::OutputTooLarge {
                stream: "stdout" | "stderr",
                ..
            }) | Err(ProbeError::GroupSignalFailed(_))
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn kills_descendant_holding_pipe_without_stale_pgid_signal() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = std::env::temp_dir().join(format!("tau-probe-descendant-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let script = dir.join("ffprobe");
        fs::write(
            &script,
            "#!/bin/sh\necho $$ > \"${10}.pid\"\n(sleep 1; echo survived > \"${10}.late\") &\nprintf '{\\\"streams\\\":[],\\\"format\\\":{}}'\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let marker = dir.join("descendant-marker");
        let result = run_probe(&script, &marker);
        assert!(result.is_ok(), "unexpected result: {result:?}");
        let pid: i32 = fs::read_to_string(format!("{}.pid", marker.display()))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            matches!(
                rustix::process::waitpid(
                    Some(rustix::process::Pid::from_raw(pid).unwrap()),
                    rustix::process::WaitOptions::NOHANG
                ),
                Err(e) if e == rustix::io::Errno::CHILD
            ),
            "the direct child should already be reaped"
        );
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!dir.join("descendant-marker.late").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn reports_descendant_that_escapes_its_process_group() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = std::env::temp_dir().join(format!("tau-probe-escaped-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        let script = dir.join("ffprobe");
        fs::write(
            &script,
            "#!/bin/sh\npython3 -c 'import os,time; os.setsid(); time.sleep(0.8)' &\nprintf '{\\\"streams\\\":[],\\\"format\\\":{}}'\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            let _ = fs::remove_dir_all(dir);
            return;
        }
        let start = Instant::now();
        assert!(matches!(
            run_probe(&script, Path::new("unused")),
            Err(ProbeError::DescendantPipeLeak | ProbeError::GroupSignalFailed(_))
        ));
        assert!(start.elapsed() < Duration::from_millis(700));
        std::thread::sleep(Duration::from_millis(900));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn command_classifies_missing_tool_nonzero_exit_and_oversize_output() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = std::env::temp_dir().join(format!("tau-probe-errors-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        assert!(matches!(
            run_probe(&dir.join("missing"), Path::new("x")),
            Err(ProbeError::Spawn(_))
        ));

        let script = dir.join("fail");
        fs::write(&script, "#!/bin/sh\necho deliberate-failure >&2\nexit 7\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(run_probe(&script, Path::new("x")), Err(ProbeError::ToolFailed { status: Some(7), stderr, .. }) if stderr.contains("deliberate-failure"))
        );

        fs::write(&script, "#!/bin/sh\nhead -c 2097152 /dev/zero\nprintf '{\\\"streams\\\":[],\\\"format\\\":{}}'\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            run_probe(&script, Path::new("x")),
            Err(ProbeError::OutputTooLarge {
                stream: "stdout",
                ..
            })
        ));

        fs::write(
            &script,
            "#!/bin/sh\nhead -c 32768 /dev/zero >&2\nprintf '{\\\"streams\\\":[],\\\"format\\\":{}}'\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            run_probe(&script, Path::new("x")),
            Err(ProbeError::OutputTooLarge {
                stream: "stderr",
                ..
            }) | Err(ProbeError::GroupSignalFailed(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn real_ffprobe_smoke_when_ffmpeg_is_installed() {
        use std::process::Command;
        if !Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tau-probe-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let input = dir.join("tiny generated video.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=32x24:r=10",
                "-t",
                "0.2",
                "-c:v",
                "mpeg4",
                "-y",
            ])
            .arg(&input)
            .status();
        let Ok(status) = generated else {
            let _ = std::fs::remove_dir_all(dir);
            return;
        };
        if !status.success() {
            let _ = std::fs::remove_dir_all(dir);
            return;
        }
        let report = run_probe(Path::new("ffprobe"), &input).unwrap();
        assert_eq!(report.video.len(), 1);
        assert_eq!(report.video[0].width, Some(32));
        assert_eq!(report.video[0].height, Some(24));
        assert_eq!(report.estimated_output_bytes, None);
        assert_eq!(report.estimated_encode_seconds, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}

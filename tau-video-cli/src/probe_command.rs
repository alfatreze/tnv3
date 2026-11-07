//! Partial source metadata probe for the standalone CLI.
use crate::probe::ProbeError;
use std::{
    ffi::{OsStr, OsString},
    io::Write,
    path::PathBuf,
    process::ExitCode,
};

fn parse(
    args: &[OsString],
) -> Result<(PathBuf, PathBuf, Option<PathBuf>, bool, Option<PathBuf>), (u8, String)> {
    let usage = || {
        (
            2,
            "Usage: tau-codecs-cli probe INPUT [--ffmpeg PATH] [--ffprobe PATH] [--tvc-encoder PATH] [--sample-estimate] [--json]"
                .to_string(),
        )
    };
    if args.len() < 2 || args[1].to_string_lossy().starts_with('-') {
        return Err(usage());
    }
    let input = PathBuf::from(&args[1]);
    let mut selected = None;
    let mut selected_ffmpeg = None;
    let mut json_seen = false;
    let mut sample_estimate = false;
    let mut selected_tvc = None;
    let mut at = 2;
    while at < args.len() {
        if args[at] == OsStr::new("--json") && !json_seen {
            json_seen = true;
            at += 1;
        } else if args[at] == OsStr::new("--ffprobe")
            && selected.is_none()
            && at + 1 < args.len()
            && !args[at + 1].to_string_lossy().starts_with('-')
        {
            selected = Some(crate::resolve_executable_arg(PathBuf::from(&args[at + 1])));
            at += 2;
        } else if args[at] == OsStr::new("--ffmpeg")
            && selected_ffmpeg.is_none()
            && at + 1 < args.len()
            && !args[at + 1].to_string_lossy().starts_with('-')
        {
            selected_ffmpeg = Some(crate::resolve_executable_arg(PathBuf::from(&args[at + 1])));
            at += 2;
        } else if args[at] == OsStr::new("--sample-estimate") && !sample_estimate {
            sample_estimate = true;
            at += 1;
        } else if args[at] == OsStr::new("--tvc-encoder")
            && selected_tvc.is_none()
            && at + 1 < args.len()
            && !args[at + 1].to_string_lossy().starts_with('-')
        {
            selected_tvc = Some(crate::resolve_executable_arg(PathBuf::from(&args[at + 1])));
            at += 2;
        } else {
            return Err(usage());
        }
    }
    let ffprobe = selected
        .or_else(|| crate::adjacent_executable("ffprobe"))
        .or_else(|| crate::on_path("ffprobe"))
        .ok_or_else(|| (3, "FFprobe not found; use --ffprobe".to_owned()))?;
    let ffmpeg = selected_ffmpeg
        .or_else(|| crate::adjacent_executable("ffmpeg"))
        .or_else(|| crate::on_path("ffmpeg"));
    let tvc = selected_tvc.or_else(|| {
        let root = crate::discover_codec_root();
        crate::select_codec_tool(root.as_deref(), false, "tvc_enc", "tools/tvc_ref/tvc_enc")
    });
    if sample_estimate && ffmpeg.is_none() {
        return Err((
            3,
            "FFmpeg not found; sample estimate requires --ffmpeg".to_owned(),
        ));
    }
    if sample_estimate && tvc.is_none() {
        return Err((
            3,
            "TVC encoder not found; sample estimate requires --tvc-encoder".to_owned(),
        ));
    }
    Ok((input, ffprobe, ffmpeg, sample_estimate, tvc))
}

pub fn run(args: &[OsString]) -> ExitCode {
    let json = args.iter().any(|arg| arg == OsStr::new("--json"));
    let result = parse(args).and_then(|(input, ffprobe, ffmpeg, sample_estimate, tvc)| {
        crate::probe::run_probe_with_tools(&ffprobe, ffmpeg.as_deref(), &input)
            .and_then(|mut report| {
                if sample_estimate {
                    crate::probe::apply_sample_estimate(
                        &mut report,
                        &input,
                        ffmpeg.as_deref().expect("sample estimate requires FFmpeg"),
                        tvc.as_deref()
                            .expect("sample estimate requires TVC encoder"),
                    )?;
                }
                Ok(report)
            })
            .map_err(|e| {
                let code = match e {
                    ProbeError::ToolFailed { .. }
                    | ProbeError::InvalidJson
                    | ProbeError::InvalidShape
                    | ProbeError::TooLarge
                    | ProbeError::OutputTooLarge { .. } => 1,
                    _ => 3,
                };
                (code, e.to_string())
            })
    });
    let (status, line) = match result {
        Ok(report) => {
            let value = report.to_json_value();
            let line = if json {
                value.to_string()
            } else {
                format!(
                    "Source format: {}; duration: {} seconds; video streams: {}; audio streams: {}. Default reference geometry: {}; minimum geometry level {}; video sample estimate: {} bytes, {} seconds (sampled video only).",
                    report.format_name.as_deref().unwrap_or("unknown"),
                    report
                        .duration_seconds
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    report.video.len(),
                    report.audio.len(),
                    report
                        .output_geometry
                        .map(|g| format!("{}x{}", g.0, g.1))
                        .unwrap_or_else(|| report
                            .output_geometry_error
                            .clone()
                            .unwrap_or_else(|| "unavailable".into())),
                    report
                        .output_level
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    report
                        .estimated_video_payload_bytes
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    report
                        .estimated_video_stage_seconds
                        .map(|v| format!("{v:.2}"))
                        .unwrap_or_else(|| "unavailable".to_owned())
                )
            };
            (0, line)
        }
        Err((code, message)) => {
            if !json {
                let _ = writeln!(std::io::stderr().lock(), "{message}");
                return ExitCode::from(code);
            }
            (code,serde_json::json!({"schema":1,"command":"probe","error":{"code":code,"message":message}}).to_string())
        }
    };
    if crate::write_stdout_line(&line).is_err() {
        let _ = writeln!(std::io::stderr().lock(), "Could not write probe report");
        return ExitCode::from(3);
    }
    ExitCode::from(status)
}

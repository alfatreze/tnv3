use std::process::Command;

#[test]
fn probe_rejects_invalid_arguments_with_json_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "input", "--json", "--bad"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["error"]["code"], 2);
    let missing_value = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "input", "--ffprobe", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing_value.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&missing_value.stdout).unwrap();
    assert_eq!(report["error"]["code"], 2);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn probe_reports_metadata_geometry_and_missing_estimates() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = std::env::temp_dir().join(format!("tau-probe-cli-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let tool = dir.join("fake-ffprobe");
    fs::write(&tool, "#!/bin/sh\nprintf '%s' '{\"streams\":[{\"index\":0,\"codec_type\":\"video\",\"codec_name\":\"h264\",\"width\":640,\"height\":360,\"sample_aspect_ratio\":\"4:3\",\"display_aspect_ratio\":\"16:9\",\"side_data_list\":[{\"rotation\":90}]},{\"index\":1,\"codec_type\":\"audio\",\"codec_name\":\"aac\"},{\"index\":2,\"codec_type\":\"video\",\"codec_name\":\"h264\",\"width\":1920,\"height\":1080}],\"format\":{\"duration\":\"1.25\"}}'\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    let ffmpeg = dir.join("fake-ffmpeg");
    fs::write(
        &ffmpeg,
        "#!/bin/sh\nprintf 'YUV4MPEG2 W64 H48 F24:1 C420jpeg\\nFRAME\\n'\nhead -c 4608 /dev/zero\n",
    )
    .unwrap();
    fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o700)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "input.mp4", "--ffprobe"])
        .arg(&tool)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["command"], "probe");
    assert_eq!(
        report["scope"],
        "source-metadata-and-default-profile-first-frame"
    );
    assert_eq!(report["packets_checked"], false);
    assert_eq!(
        report["output_profile"]["name"],
        "pocket-default-reference-400x360-24fps"
    );
    assert_eq!(report["output_geometry"]["width"], 64);
    assert_eq!(report["output_geometry"]["height"], 48);
    assert_eq!(report["output_level"]["minimum_fitting"], 1);
    assert_eq!(report["video"][0]["width"], 640);
    assert_eq!(report["video"][0]["sample_aspect_ratio"], "4:3");
    assert_eq!(report["video"][0]["rotation_degrees"].as_f64(), Some(90.0));
    assert_eq!(report["video"].as_array().unwrap().len(), 2);
    assert_eq!(report["output_profile"]["selected_video_stream_index"], 0);
    assert_eq!(report["estimate"]["status"], "unavailable");
    assert!(report["estimate"]["encode_seconds"].is_null());
    let missing = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "input", "--ffprobe"])
        .arg(dir.join("missing"))
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(report["error"]["code"], 3);

    // Missing FFmpeg must not discard usable FFprobe metadata.
    let no_ffmpeg = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "input.mp4", "--ffprobe"])
        .arg(&tool)
        .args(["--ffmpeg"])
        .arg(dir.join("missing-ffmpeg"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(no_ffmpeg.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&no_ffmpeg.stdout).unwrap();
    assert_eq!(report["video"][0]["sample_aspect_ratio"], "4:3");
    assert!(report["output_geometry"].is_null());
    assert!(!report["output_geometry_error"].as_str().unwrap().is_empty());

    // A valid audio-only source similarly returns metadata and a geometry error.
    let audio_probe = dir.join("audio-ffprobe");
    fs::write(&audio_probe, "#!/bin/sh\nprintf '%s' '{\"streams\":[{\"index\":0,\"codec_type\":\"audio\",\"codec_name\":\"aac\"}],\"format\":{}}'\n").unwrap();
    fs::set_permissions(&audio_probe, fs::Permissions::from_mode(0o700)).unwrap();
    let failing_ffmpeg = dir.join("failing-ffmpeg");
    fs::write(
        &failing_ffmpeg,
        "#!/bin/sh\nprintf 'no video stream' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&failing_ffmpeg, fs::Permissions::from_mode(0o700)).unwrap();
    let audio_only = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "audio.mp4", "--ffprobe"])
        .arg(&audio_probe)
        .args(["--ffmpeg"])
        .arg(&failing_ffmpeg)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(audio_only.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&audio_only.stdout).unwrap();
    assert!(report["video"].as_array().unwrap().is_empty());
    assert_eq!(report["audio"].as_array().unwrap().len(), 1);
    assert!(report["output_geometry"].is_null());
    assert!(report["output_geometry_error"].is_string());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn sample_estimate_is_opt_in_measured_and_explicitly_video_only() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = std::env::temp_dir().join(format!("tau-probe-estimate-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let ffprobe = dir.join("ffprobe");
    fs::write(
        &ffprobe,
        "#!/bin/sh\nprintf '%s' '{\"streams\":[{\"index\":0,\"codec_type\":\"video\",\"codec_name\":\"h264\",\"width\":64,\"height\":48,\"avg_frame_rate\":\"24/1\",\"duration\":\"10\"}],\"format\":{\"duration\":\"20\"}}'\n",
    )
    .unwrap();
    let ffmpeg = dir.join("ffmpeg");
    fs::write(
        &ffmpeg,
        "#!/bin/sh\nprintf 'YUV4MPEG2 W64 H48 F24:1 C420jpeg\\n'; for i in 1 2 3; do printf 'FRAME\\n'; dd if=/dev/zero bs=4608 count=1 2>/dev/null; done\n",
    )
    .unwrap();
    let encoder = dir.join("tvc_enc");
    fs::write(&encoder, "#!/bin/sh\nhead -c 1000 /dev/zero > \"$2\"\n").unwrap();
    for tool in [&ffprobe, &ffmpeg, &encoder] {
        fs::set_permissions(tool, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "fixture.mp4", "--sample-estimate", "--ffprobe"])
        .arg(&ffprobe)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--tvc-encoder")
        .arg(&encoder)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let estimate = &report["estimate"];
    assert_eq!(estimate["status"], "sampled_video_only");
    assert_eq!(estimate["sample_frames"], 3);
    assert_eq!(estimate["sample_output_bytes"], 1000);
    assert_eq!(estimate["sample_settings"]["qp"], 30);
    assert_eq!(
        estimate["projection_duration_basis"],
        "first_video_stream_duration"
    );
    assert_eq!(estimate["video_payload_bytes"], 80000);
    assert!(estimate["video_stage_seconds"].as_f64().unwrap() > 0.0);
    assert!(estimate["output_bytes"].is_null());
    assert!(estimate["encode_seconds"].is_null());
    assert!(
        estimate["scope"]
            .as_str()
            .unwrap()
            .contains("excludes TNV3")
    );
    let human = Command::new(env!("CARGO_BIN_EXE_tau-codecs-cli"))
        .args(["probe", "fixture.mp4", "--sample-estimate", "--ffprobe"])
        .arg(&ffprobe)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--tvc-encoder")
        .arg(&encoder)
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(0));
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("video sample estimate: 80000 bytes"),
        "{text}"
    );
    assert!(text.contains("seconds (sampled video only"), "{text}");
    fs::remove_dir_all(dir).unwrap();
}

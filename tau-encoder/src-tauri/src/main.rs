#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod player;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
#[cfg(feature = "experimental-rust-encoder")]
use std::{
    io::BufReader,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn temporary_file(ext: &str) -> Result<(Temporary, File), String> {
    for _ in 0..100 {
        let name = format!(
            "tau-encoder-{}-{}.{ext}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create temporary file: {e}")),
        }
    }
    Err("Could not reserve a temporary file.".into())
}
#[derive(Serialize)]
struct ResultInfo {
    frames: u32,
    level: u8,
    reencoded: u32,
    video: String,
    audio: String,
    warnings: Vec<String>,
}
fn safe_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with([' ', '.'])
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !matches!(
            name.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
fn process_failed(status: std::process::ExitStatus, action: &str) -> String {
    format!(
        "{action} failed ({status}). Check that the source has a supported video and audio track."
    )
}
// Drain the entire pipe to avoid blocking FFmpeg, retaining a bounded excerpt.
fn capture_diagnostics(
    mut stderr: impl Read + Send + 'static,
) -> std::thread::JoinHandle<io::Result<String>> {
    std::thread::spawn(move || {
        const LIMIT: usize = 4096;
        let mut saved = Vec::new();
        let mut buf = [0u8; 4096];
        let mut truncated = false;
        loop {
            let n = stderr.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let take = n.min(LIMIT.saturating_sub(saved.len()));
            saved.extend_from_slice(&buf[..take]);
            if take < n {
                truncated = true;
            }
        }
        let mut text = String::from_utf8_lossy(&saved).trim().to_owned();
        if truncated {
            text.push_str("\n[Further diagnostics omitted.]");
        }
        Ok(text)
    })
}
fn finish_diagnostics(
    handle: std::thread::JoinHandle<io::Result<String>>,
) -> Result<String, String> {
    handle
        .join()
        .map_err(|_| "Could not collect decoder diagnostics.".to_owned())?
        .map_err(|e| format!("Could not read decoder diagnostics: {e}"))
}
fn with_diagnostics(message: String, details: &str) -> String {
    if details.is_empty() {
        message
    } else {
        format!("{message}\n{details}")
    }
}

#[derive(Clone, Serialize, Debug)]
struct Progress {
    phase: &'static str,
    frames: u32,
    total_frames_estimate: Option<u32>,
    remaining_seconds: Option<u64>,
}

fn parse_duration(text: &str) -> Option<f64> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Duration: "))?
        .split(',')
        .next()?;
    let mut parts = value.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || !(0.0..60.0).contains(&minutes)
        || !(0.0..60.0).contains(&seconds)
        || !(0.0..=4.0).contains(&hours)
    {
        return None;
    }
    let duration = hours * 3600.0 + minutes * 60.0 + seconds;
    if duration > 0.0 && duration <= 14400.0 {
        Some(duration)
    } else {
        None
    }
}

fn estimated_frames(ffmpeg: &Path, input: &Path) -> Option<u32> {
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-i"])
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut buf = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(65536).read_to_end(&mut buf);
    }
    let _ = child.wait();
    let seconds = parse_duration(&String::from_utf8_lossy(&buf))?;
    Some((seconds * 24.0).ceil() as u32)
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AdvancedSettings {
    round_down: bool,
    eof_pass: bool,
    trim_mp3_delay: bool,
}
impl AdvancedSettings {
    #[cfg(feature = "experimental-rust-encoder")]
    fn fps_filter(self) -> &'static str {
        match (self.round_down, self.eof_pass) {
            (false, _) => "fps=24",
            (true, false) => "fps=24:round=down",
            (true, true) => "fps=24:round=down:eof_action=pass",
        }
    }
    fn audio_filter(self) -> &'static str {
        if self.trim_mp3_delay {
            "atrim=start_sample=1105,asetpts=PTS-STARTPTS,apad"
        } else {
            "apad"
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Dependencies {
    ffmpeg: String,
    ffprobe: String,
    python: String,
    script: String,
    tvc_encoder: String,
    tnv3_tool: String,
}
fn selected_file(value: &str, label: &str) -> Result<Option<PathBuf>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() || !path.is_file() {
        return Err(format!(
            "{label}: choose an existing file using its full path in Dependency settings."
        ));
    }
    Ok(Some(path))
}

struct VideoSummary {
    frames: u32,
    level: u8,
    reencoded: u32,
}

#[allow(clippy::too_many_arguments)] // Preserve the explicit app-to-pipeline configuration boundary.
fn encode_pair(
    ffmpeg: &Path,
    input: &Path,
    folder: &Path,
    name: &str,
    qp: u8,
    c_tools: Option<(&Path, &Path)>,
    advanced: AdvancedSettings,
    dependencies: &Dependencies,
    mut progress: impl FnMut(Progress),
) -> Result<ResultInfo, String> {
    if qp > 51 || !safe_name(name) {
        return Err("Choose a valid output name and a QP between 0 and 51.".into());
    }
    let video_path = folder.join(format!("{name}.tnv3"));
    let audio_path = folder.join(format!("{name}.mp3"));
    if video_path.exists() || audio_path.exists() {
        return Err("An output file already exists. Choose a different name.".into());
    }
    progress(Progress {
        phase: "probing",
        frames: 0,
        total_frames_estimate: None,
        remaining_seconds: None,
    });
    let total = estimated_frames(ffmpeg, input);
    progress(Progress {
        phase: "video",
        frames: 0,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    let mut warnings = Vec::new();
    let (_video_tmp, mut tvc) = temporary_file(if c_tools.is_some() { "tnv3" } else { "tvc" })?;
    let stats = if let Some((script, ffprobe)) = c_tools {
        progress(Progress {
            phase: "c-video",
            frames: 0,
            total_frames_estimate: None,
            remaining_seconds: None,
        });
        let jobs = std::thread::available_parallelism().map_or(1, |n| n.get().min(6));
        let python = selected_file(&dependencies.python, "Python")?
            .map(|p| p.into_os_string())
            .or_else(|| std::env::var_os("TAU_PYTHON"))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "python".into()
                } else {
                    "python3".into()
                }
            });
        let tools = tau_video::c_encoder::Tools {
            python: python.into(),
            script: script.to_path_buf(),
            ffmpeg: ffmpeg.to_path_buf(),
            ffprobe: ffprobe.to_path_buf(),
            tvc_encoder: selected_file(&dependencies.tvc_encoder, "TVC encoder")?,
            tnv3_tool: selected_file(&dependencies.tnv3_tool, "TNV3 tool")?,
        };
        let timing = match (advanced.round_down, advanced.eof_pass) {
            (false, _) => tau_video::c_encoder::FrameTiming::Default,
            (true, false) => tau_video::c_encoder::FrameTiming::RoundDown,
            (true, true) => tau_video::c_encoder::FrameTiming::RoundDownEofPass,
        };
        let report = tau_video::c_encoder::encode_video(
            &tools,
            input,
            &mut tvc,
            tau_video::c_encoder::Options { qp, jobs, timing },
            |done, all| {
                progress(Progress {
                    phase: "c-video",
                    frames: done,
                    total_frames_estimate: Some(all),
                    remaining_seconds: None,
                });
            },
        )?;
        if !report.diagnostics.is_empty() {
            warnings.push(with_diagnostics(
                "Source decoding reported errors; inspect the output.".into(),
                &report.diagnostics,
            ));
        }
        VideoSummary {
            frames: report.frames,
            level: report.level,
            reencoded: report.reencoded,
        }
    } else {
        #[cfg(not(feature = "experimental-rust-encoder"))]
        {
            return Err("The experimental Rust encoder is not enabled in this build.".into());
        }
        #[cfg(feature = "experimental-rust-encoder")]
        {
            let video_filter = format!(
                "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,{}",
                advanced.fps_filter()
            );
            let mut decoder = Command::new(ffmpeg)
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(input)
                .args([
                    "-map",
                    "0:v:0",
                    "-an",
                    "-sn",
                    "-vf",
                    &video_filter,
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "yuv4mpegpipe",
                    "pipe:1",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
            let stderr = decoder
                .stderr
                .take()
                .ok_or("Could not capture source diagnostics.")?;
            let log = capture_diagnostics(stderr);
            let stdout = decoder
                .stdout
                .take()
                .ok_or("Could not read FFmpeg video output.")?;
            let start = Instant::now();
            let mut last_update = start;
            let mut samples = std::collections::VecDeque::from([(start, 0u32)]);
            let encoded = tau_video::tvc::encoder::encode_y4m_with_progress(
                BufReader::new(stdout),
                &mut tvc,
                qp,
                |frames| {
                    let now = Instant::now();
                    if frames != 1 && now.duration_since(last_update) < Duration::from_millis(500) {
                        return;
                    }
                    samples.push_back((now, frames));
                    while samples.len() > 2
                        && now.duration_since(samples[1].0) > Duration::from_secs(10)
                    {
                        samples.pop_front();
                    }
                    let (first, first_frames) = samples.front().copied().unwrap_or((start, 0));
                    let seconds = now.duration_since(first).as_secs_f64();
                    let rate = f64::from(frames - first_frames) / seconds.max(0.001);
                    let remaining_seconds = total
                        .filter(|&n| frames < n && seconds >= 2.0 && rate > 0.0)
                        .map(|n| (f64::from(n - frames) / rate).ceil() as u64);
                    progress(Progress {
                        phase: "video",
                        frames,
                        total_frames_estimate: total,
                        remaining_seconds,
                    });
                    last_update = now;
                },
            );
            if encoded.is_err() {
                let _ = decoder.kill();
            }
            let status = decoder
                .wait()
                .map_err(|e| format!("Could not wait for FFmpeg: {e}"))?;
            let details = finish_diagnostics(log)?;
            let stats = encoded
                .map_err(|e| with_diagnostics(format!("Video encoding failed: {e}"), &details))?;
            if !status.success() {
                return Err(with_diagnostics(
                    process_failed(status, "Video conversion"),
                    &details,
                ));
            }
            if !details.is_empty() {
                warnings.push(with_diagnostics(
                    "Source decoding reported errors; inspect the output.".into(),
                    &details,
                ));
            }
            VideoSummary {
                frames: stats.frames,
                level: stats.level,
                reencoded: stats.reencoded,
            }
        }
    };
    progress(Progress {
        phase: "audio",
        frames: stats.frames,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    let (audio_tmp, audio_file) = temporary_file("mp3")?;
    drop(audio_file);
    let duration = format!("{:.9}", f64::from(stats.frames) / 24.0);
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-map_metadata",
            "-1",
            "-af",
            advanced.audio_filter(),
            "-t",
            &duration,
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
            "-f",
            "mp3",
        ])
        .arg(&audio_tmp.0)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Could not capture audio diagnostics.")?;
    let log = capture_diagnostics(stderr);
    let status = child
        .wait()
        .map_err(|e| format!("Could not wait for FFmpeg: {e}"))?;
    let details = finish_diagnostics(log)?;
    if !status.success() {
        return Err(with_diagnostics(
            process_failed(status, "MP3 encoding"),
            &details,
        ));
    }
    if !details.is_empty() {
        warnings.push(with_diagnostics(
            "Audio decoding reported errors; inspect the output.".into(),
            &details,
        ));
    }
    progress(Progress {
        phase: "saving",
        frames: stats.frames,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    tvc.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Could not read temporary video: {e}"))?;
    let mut video = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&video_path)
        .map_err(|e| format!("Could not create video output: {e}"))?;
    let result = (|| {
        let mut audio = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&audio_path)
            .map_err(|e| format!("Could not create audio output: {e}"))?;
        let result = (|| {
            if c_tools.is_some() {
                io::copy(&mut tvc, &mut video).map_err(|e| format!("Could not copy TNV3: {e}"))?;
            } else {
                tau_video::container::mux_tvc(&mut tvc, &mut video)
                    .map_err(|e| format!("Could not create TNV3: {e}"))?;
            }
            io::copy(
                &mut File::open(&audio_tmp.0).map_err(|e| e.to_string())?,
                &mut audio,
            )
            .map_err(|e| format!("Could not write MP3: {e}"))?;
            video
                .sync_all()
                .and_then(|_| audio.sync_all())
                .map_err(|e| format!("Could not finish output files: {e}"))?;
            Ok(())
        })();
        drop(audio);
        if result.is_err() {
            std::fs::remove_file(&audio_path)
                .map_err(|e| format!("Could not remove incomplete MP3: {e}"))?;
        }
        result
    })();
    drop(video);
    if let Err(e) = result {
        return match std::fs::remove_file(&video_path) {
            Ok(()) => Err(e),
            Err(cleanup) => Err(format!("{e} Could not remove incomplete video: {cleanup}")),
        };
    }
    Ok(ResultInfo {
        frames: stats.frames,
        level: stats.level,
        reencoded: stats.reencoded,
        video: video_path.display().to_string(),
        audio: audio_path.display().to_string(),
        warnings,
    })
}
fn codec_script(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join("codec-tools/tnv3_ref/encode_film.py");
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/tnv3_ref/encode_film.py")
}
fn ffmpeg_path(app: &tauri::AppHandle) -> PathBuf {
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join("ffmpeg").join(exe);
        if p.is_file() {
            return p;
        }
    }
    if let Some(p) = std::env::var_os("TAU_FFMPEG") {
        return p.into();
    }
    #[cfg(target_os = "macos")]
    for name in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
        if Path::new(name).is_file() {
            return name.into();
        }
    }
    exe.into()
}
#[tauri::command]
fn experimental_rust_encoder_available() -> bool {
    cfg!(feature = "experimental-rust-encoder")
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Parameters are named Tauri command arguments.
async fn encode(
    app: tauri::AppHandle,
    input: String,
    folder: String,
    name: String,
    qp: u8,
    mode: String,
    advanced: AdvancedSettings,
    dependencies: Dependencies,
    busy: tauri::State<'_, Mutex<bool>>,
) -> Result<ResultInfo, String> {
    let chosen_ffmpeg = selected_file(&dependencies.ffmpeg, "FFmpeg")?;
    let chosen_ffprobe = if mode == "c" {
        selected_file(&dependencies.ffprobe, "FFprobe")?
    } else {
        None
    };
    let chosen_script = if mode == "c" {
        selected_file(&dependencies.script, "C encoder script")?
    } else {
        None
    };
    {
        let mut running = busy.lock().map_err(|_| "Encoder state unavailable.")?;
        if *running {
            return Err("An encode is already running.".into());
        }
        *running = true;
    }
    if mode != "c" && !(mode == "rust" && cfg!(feature = "experimental-rust-encoder")) {
        *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
        return Err("Unknown encoder.".into());
    }
    let ffmpeg = chosen_ffmpeg.unwrap_or_else(|| ffmpeg_path(&app));
    let ffprobe = chosen_ffprobe.unwrap_or_else(|| {
        ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        })
    });
    let script = if mode == "c" {
        Some(chosen_script.unwrap_or_else(|| codec_script(&app)))
    } else {
        None
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let reporter = app.clone();
        encode_pair(
            &ffmpeg,
            Path::new(&input),
            Path::new(&folder),
            &name,
            qp,
            script.as_deref().map(|s| (s, ffprobe.as_path())),
            advanced,
            &dependencies,
            move |event| {
                let _ = reporter.emit("encode-progress", event);
            },
        )
    })
    .await;
    *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
    result.map_err(|e| format!("Encoder task stopped: {e}"))?
}
#[tauri::command]
fn player_open(
    app: tauri::AppHandle,
    path: String,
    state: tauri::State<'_, Mutex<Option<player::Player>>>,
) -> Result<player::PlayerInfo, String> {
    let (player, info) = player::Player::open(Path::new(&path))?;
    if let Some(ref audio) = info.audio {
        app.asset_protocol_scope()
            .allow_file(audio)
            .map_err(|e| e.to_string())?;
    }
    *state.lock().map_err(|_| "Player state unavailable.")? = Some(player);
    Ok(info)
}
#[tauri::command]
async fn player_next(app: tauri::AppHandle) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<Option<player::Player>>>();
        let mut locked = state.lock().map_err(|_| "Player state unavailable.")?;
        let Some(p) = locked.as_mut() else {
            return Err("Open a video first.".to_owned());
        };
        let Some(frame) = p.next_frame()? else {
            return Ok(tauri::ipc::Response::new(Vec::<u8>::new()));
        };
        let mut data = frame.y;
        data.extend_from_slice(&frame.cb);
        data.extend_from_slice(&frame.cr);
        Ok(tauri::ipc::Response::new(data))
    })
    .await
    .map_err(|e| format!("Preview stopped: {e}"))?
}
#[tauri::command]
fn player_close(state: tauri::State<'_, Mutex<Option<player::Player>>>) -> Result<(), String> {
    *state.lock().map_err(|_| "Player state unavailable.")? = None;
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(false))
        .manage(Mutex::new(None::<player::Player>))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            experimental_rust_encoder_available,
            encode,
            player_open,
            player_next,
            player_close
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tau Encoder");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_probe_handles_valid_and_unknown_lengths() {
        assert_eq!(
            parse_duration("  Duration: 01:02:03.50, start: 0.0"),
            Some(3723.5)
        );
        assert_eq!(parse_duration("  Duration: N/A, start: 0.0"), None);
        assert_eq!(parse_duration("  Duration: 00:99:00.00,"), None);
        assert_eq!(parse_duration("  Duration: 06:00:00.00,"), None);
    }
    #[test]
    fn diagnostic_capture_is_bounded_and_drains_the_input() {
        let data = vec![b'x'; 100_000];
        let result = finish_diagnostics(capture_diagnostics(std::io::Cursor::new(data))).unwrap();
        assert!(result.len() < 4200);
        assert!(result.ends_with("[Further diagnostics omitted.]"));
    }
    #[cfg(feature = "experimental-rust-encoder")]
    #[test]
    fn reported_source_corruption_is_returned_as_warning() {
        let (Some(source), Some(ffmpeg), Some(script), Some(probe)) = (
            std::env::var_os("TAU_TEST_CORRUPT_HEVC"),
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_C_SCRIPT"),
            std::env::var_os("TAU_TEST_FFPROBE"),
        ) else {
            return;
        };
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        for (suffix, c_tools) in [
            ("_r", None),
            ("_c", Some((Path::new(&script), Path::new(&probe)))),
        ] {
            let result = encode_pair(
                Path::new(&ffmpeg),
                Path::new(&source),
                folder,
                &format!("{name}{suffix}"),
                30,
                c_tools,
                AdvancedSettings::default(),
                &Dependencies::default(),
                |_| {},
            )
            .unwrap();
            let _video = Temporary(PathBuf::from(result.video));
            let _audio = Temporary(PathBuf::from(result.audio));
            assert!(
                !result.warnings.is_empty(),
                "reported corruption must not be silent: {suffix}"
            );
            assert!(result.warnings[0].contains("Source decoding reported errors"));
        }
    }
    #[cfg(feature = "experimental-rust-encoder")]
    #[test]
    fn reported_audio_errors_are_returned_as_warning() {
        let (Some(source), Some(ffmpeg)) = (
            std::env::var_os("TAU_TEST_CORRUPT_AUDIO"),
            std::env::var_os("TAU_TEST_FFMPEG"),
        ) else {
            return;
        };
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        let result = encode_pair(
            Path::new(&ffmpeg),
            Path::new(&source),
            folder,
            name,
            30,
            None,
            AdvancedSettings::default(),
            &Dependencies::default(),
            |_| {},
        )
        .unwrap();
        let _video = Temporary(PathBuf::from(result.video));
        let _audio = Temporary(PathBuf::from(result.audio));
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Audio decoding reported errors"))
        );
    }
    #[cfg(feature = "experimental-rust-encoder")]
    #[test]
    fn experimental_settings_apply_to_both_encoders() {
        let (Some(source), Some(ffmpeg), Some(script), Some(probe)) = (
            std::env::var_os("TAU_TEST_MARKER"),
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_C_SCRIPT"),
            std::env::var_os("TAU_TEST_FFPROBE"),
        ) else {
            return;
        };
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let folder = reserve.0.parent().unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        for (backend, tools) in [
            ("rust", None),
            ("c", Some((Path::new(&script), Path::new(&probe)))),
        ] {
            let mut baseline_pulse = None;
            for (label, settings) in [
                ("default", AdvancedSettings::default()),
                (
                    "down",
                    AdvancedSettings {
                        round_down: true,
                        ..Default::default()
                    },
                ),
                (
                    "pass",
                    AdvancedSettings {
                        round_down: true,
                        eof_pass: true,
                        ..Default::default()
                    },
                ),
                (
                    "trim",
                    AdvancedSettings {
                        trim_mp3_delay: true,
                        ..Default::default()
                    },
                ),
            ] {
                let output = encode_pair(
                    Path::new(&ffmpeg),
                    Path::new(&source),
                    folder,
                    &format!("{name}_{backend}_{label}"),
                    30,
                    tools,
                    settings,
                    &Dependencies::default(),
                    |_| {},
                )
                .unwrap();
                let _video = Temporary(PathBuf::from(&output.video));
                let _audio = Temporary(PathBuf::from(&output.audio));
                let filter = format!(
                    "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,{}",
                    settings.fps_filter()
                );
                let expected = Command::new(&ffmpeg)
                    .args(["-v", "error", "-i"])
                    .arg(&source)
                    .args(["-map", "0:v:0", "-vf", &filter, "-f", "framehash", "-"])
                    .output()
                    .unwrap();
                assert!(expected.status.success());
                let frames = String::from_utf8(expected.stdout)
                    .unwrap()
                    .lines()
                    .filter(|l| !l.starts_with('#'))
                    .count();
                assert_eq!(output.frames as usize, frames, "{backend} {label}");
                let pcm = Command::new(&ffmpeg)
                    .args(["-v", "error", "-i"])
                    .arg(&output.audio)
                    .args(["-ac", "2", "-ar", "48000", "-f", "s16le", "-"])
                    .output()
                    .unwrap();
                assert!(pcm.status.success());
                let pulse = pcm
                    .stdout
                    .chunks_exact(4)
                    .position(|pair| {
                        i16::from_le_bytes([pair[0], pair[1]]).unsigned_abs() > 1000
                            || i16::from_le_bytes([pair[2], pair[3]]).unsigned_abs() > 1000
                    })
                    .unwrap();
                if label == "default" {
                    baseline_pulse = Some(pulse);
                }
                if label == "trim" {
                    assert_eq!(baseline_pulse.unwrap() - pulse, 1105, "{backend}");
                }
            }
        }
    }
    #[test]
    fn dependency_settings_validate_and_deserialize() {
        assert!(selected_file("", "FFmpeg").unwrap().is_none());
        assert!(
            selected_file("relative/path", "FFmpeg")
                .unwrap_err()
                .contains("FFmpeg")
        );
        let value: Dependencies = serde_json::from_str(r#"{"ffmpeg":"a","ffprobe":"b","python":"c","script":"d","tvcEncoder":"e","tnv3Tool":"f"}"#).unwrap();
        assert_eq!(
            (
                value.ffmpeg.as_str(),
                value.ffprobe.as_str(),
                value.python.as_str(),
                value.script.as_str(),
                value.tvc_encoder.as_str(),
                value.tnv3_tool.as_str()
            ),
            ("a", "b", "c", "d", "e", "f")
        );
    }
    #[test]
    fn configured_dependency_paths_work_with_separate_c_tools() {
        let (
            Some(source),
            Some(ffmpeg),
            Some(ffprobe),
            Some(script),
            Some(python),
            Some(tvc),
            Some(tnv3),
        ) = (
            std::env::var_os("TAU_TEST_MARKER"),
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_FFPROBE"),
            std::env::var_os("TAU_TEST_C_SCRIPT"),
            std::env::var_os("TAU_TEST_PYTHON"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        )
        else {
            return;
        };
        assert!(selected_file("", "FFmpeg").unwrap().is_none());
        assert!(
            selected_file("not/a/full/path", "FFmpeg")
                .unwrap_err()
                .contains("FFmpeg")
        );
        assert!(selected_file("/does/not/exist", "Python").is_err());
        let (script_guard, script_file) = temporary_file("py").unwrap();
        drop(script_file);
        std::fs::copy(script, &script_guard.0).unwrap(); // No helper binaries next to this copy.
        let dependencies = Dependencies {
            ffmpeg: PathBuf::from(&ffmpeg).display().to_string(),
            ffprobe: PathBuf::from(&ffprobe).display().to_string(),
            python: PathBuf::from(python).display().to_string(),
            script: script_guard.0.display().to_string(),
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
        };
        let f = selected_file(&dependencies.ffmpeg, "FFmpeg")
            .unwrap()
            .unwrap();
        let probe = selected_file(&dependencies.ffprobe, "FFprobe")
            .unwrap()
            .unwrap();
        let script = selected_file(&dependencies.script, "script")
            .unwrap()
            .unwrap();
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let result = encode_pair(
            &f,
            Path::new(&source),
            reserve.0.parent().unwrap(),
            name,
            30,
            Some((&script, &probe)),
            AdvancedSettings::default(),
            &dependencies,
            |_| {},
        )
        .unwrap();
        let _video = Temporary(PathBuf::from(result.video));
        let _audio = Temporary(PathBuf::from(result.audio));
        assert!(result.frames > 0);
    }
    #[test]
    fn names_are_safe_on_all_platforms() {
        assert!(safe_name("My film"));
        for name in ["", "..", "CON", "a/b", "bad.", "a\\b", "x:y"] {
            assert!(!safe_name(name));
        }
    }
    #[cfg(feature = "experimental-rust-encoder")]
    #[test]
    fn real_media_pair_and_no_overwrite() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else {
            return;
        };
        let (_src_guard, src_file) = temporary_file("mp4").unwrap();
        let src = _src_guard.0.clone();
        drop(src_file);
        let status = Command::new(&ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x48:rate=24:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=1",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ])
            .arg(&src)
            .status()
            .unwrap();
        assert!(status.success());
        let (reserve, f) = temporary_file("tnv3").unwrap();
        drop(f);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        let mut phases = Vec::new();
        let result = encode_pair(
            Path::new(&ffmpeg),
            &src,
            folder,
            name,
            30,
            None,
            AdvancedSettings::default(),
            &Dependencies::default(),
            |p| phases.push(p),
        )
        .unwrap();
        assert_eq!(phases.first().unwrap().phase, "probing");
        assert!(
            phases
                .iter()
                .any(|p| p.phase == "video" && p.total_frames_estimate == Some(24))
        );
        assert!(phases.iter().any(|p| p.phase == "audio"));
        assert_eq!(phases.last().unwrap().phase, "saving");
        assert_eq!(result.frames, 24);
        assert!(
            result.warnings.is_empty(),
            "clean input should not produce warnings"
        );
        let video_guard = Temporary(PathBuf::from(&result.video));
        let audio_guard = Temporary(PathBuf::from(&result.audio));
        assert_eq!(
            tau_video::Tnv3Reader::open(File::open(&video_guard.0).unwrap())
                .unwrap()
                .info()
                .frame_count,
            24
        );
        if let Some(checker) = std::env::var_os("TAU_TEST_TNV3") {
            assert!(
                Command::new(checker)
                    .arg("check")
                    .arg(&video_guard.0)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        if let Some(ffprobe) = std::env::var_os("TAU_TEST_FFPROBE") {
            let probe = Command::new(ffprobe)
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name,sample_rate,channels",
                    "-of",
                    "default=nw=1",
                ])
                .arg(&audio_guard.0)
                .output()
                .unwrap();
            assert!(probe.status.success());
            let details = String::from_utf8(probe.stdout).unwrap();
            assert!(
                details.contains("codec_name=mp3")
                    && details.contains("sample_rate=48000")
                    && details.contains("channels=2")
            );
        }
        let audio_bytes = std::fs::read(&audio_guard.0).unwrap();
        assert!(audio_bytes.len() > 1000);
        assert!(
            encode_pair(
                Path::new(&ffmpeg),
                &src,
                folder,
                name,
                30,
                None,
                AdvancedSettings::default(),
                &Dependencies::default(),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&audio_guard.0).unwrap(), audio_bytes);
        if let Some(script) = std::env::var_os("TAU_TEST_C_SCRIPT") {
            let c_name = format!("{name}_C");
            let probe = std::env::var_os("TAU_TEST_FFPROBE").unwrap_or_else(|| "ffprobe".into());
            let c = encode_pair(
                Path::new(&ffmpeg),
                &src,
                folder,
                &c_name,
                30,
                Some((Path::new(&script), Path::new(&probe))),
                AdvancedSettings::default(),
                &Dependencies::default(),
                |_| {},
            )
            .unwrap();
            let c_video = Temporary(PathBuf::from(&c.video));
            let _c_audio = Temporary(PathBuf::from(&c.audio));
            assert_eq!(c.frames, 24);
            assert!(
                c.warnings.is_empty(),
                "clean C input should not produce warnings"
            );
            let (mut player, info) = player::Player::open(&c_video.0).unwrap();
            assert_eq!(info.frames, 24);
            let mut actual = Vec::new();
            let mut count = 0;
            while let Some(f) = player.next_frame().unwrap() {
                actual.extend_from_slice(b"FRAME\n");
                actual.extend_from_slice(&f.y);
                actual.extend_from_slice(&f.cb);
                actual.extend_from_slice(&f.cr);
                count += 1;
            }
            assert_eq!(count, 24);
            let mut reference = Vec::new();
            tau_video::container::decode_video_y4m(File::open(&c_video.0).unwrap(), &mut reference)
                .unwrap();
            let first_line = reference.iter().position(|&b| b == b'\n').unwrap() + 1;
            assert_eq!(actual, reference[first_line..]);
        }
    }
}

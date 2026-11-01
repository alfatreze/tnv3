#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod player;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
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

fn encode_pair(
    ffmpeg: &Path,
    input: &Path,
    folder: &Path,
    name: &str,
    qp: u8,
    c_tools: Option<(&Path, &Path)>,
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
    let (_video_tmp, mut tvc) = temporary_file(if c_tools.is_some() { "tnv3" } else { "tvc" })?;
    let stats = if let Some((script, ffprobe)) = c_tools {
        progress(Progress {
            phase: "c-video",
            frames: 0,
            total_frames_estimate: None,
            remaining_seconds: None,
        });
        let (work_guard, work_file) = temporary_file("work")?;
        drop(work_file);
        std::fs::remove_file(&work_guard.0).map_err(|e| e.to_string())?;
        std::fs::create_dir(&work_guard.0).map_err(|e| e.to_string())?;
        struct WorkDir(PathBuf);
        impl Drop for WorkDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let work = WorkDir(work_guard.0.clone());
        let report_path = _video_tmp.0.with_extension("json");
        let _report = Temporary(report_path.clone());
        let (_log_guard, log_file) = temporary_file("log")?;
        let jobs = std::thread::available_parallelism().map_or(1, |n| n.get().min(6));
        let python = std::env::var_os("TAU_PYTHON").unwrap_or_else(|| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let mut child = Command::new(python)
            .arg(script)
            .arg(input)
            .arg(&_video_tmp.0)
            .args(["--video-only", "--match-app", "--qp"])
            .arg(qp.to_string())
            .args(["--jobs"])
            .arg(jobs.to_string())
            .args(["--cpu-budget"])
            .arg(jobs.to_string())
            .args(["--work"])
            .arg(&work.0)
            .env("TAU_FFMPEG", ffmpeg)
            .env("TAU_FFPROBE", ffprobe)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_file))
            .spawn()
            .map_err(|e| format!("Could not start the C encoder (Python 3 is required): {e}"))?;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(part) = line.split("encoded ").nth(1) {
                    let mut nums = part.split_whitespace();
                    if let (Some(done), Some("/"), Some(all)) =
                        (nums.next(), nums.next(), nums.next())
                    {
                        progress(Progress {
                            phase: "c-video",
                            frames: done.parse().unwrap_or(0),
                            total_frames_estimate: all.parse().ok(),
                            remaining_seconds: None,
                        });
                    }
                }
            }
        }
        let status = child
            .wait()
            .map_err(|e| format!("Could not wait for C encoder: {e}"))?;
        if !status.success() {
            let details = std::fs::read_to_string(&_log_guard.0).unwrap_or_default();
            return Err(format!(
                "C encoder failed ({status}): {}",
                details
                    .chars()
                    .rev()
                    .take(500)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            ));
        }
        let report: serde_json::Value =
            serde_json::from_reader(File::open(report_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let num = |key: &str| -> Result<u32, String> {
            report[key]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| format!("Invalid C encoder report: {key}"))
        };
        tau_video::tvc::encoder::EncodeSummary {
            frames: num("frames")?,
            level: u8::try_from(num("level")?).map_err(|_| "Invalid level")?,
            reencoded: num("pictures_raised_for_level")?,
            predicted: 0,
        }
    } else {
        let mut decoder = Command::new(ffmpeg)
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(input)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-vf",
                "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "yuv4mpegpipe",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
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
        let stats = encoded.map_err(|e| format!("Video encoding failed: {e}"))?;
        if !status.success() {
            return Err(process_failed(status, "Video conversion"));
        }
        stats
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
    let status = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-map_metadata",
            "-1",
            "-af",
            "apad",
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
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
    if !status.success() {
        return Err(process_failed(status, "MP3 encoding"));
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
async fn encode(
    app: tauri::AppHandle,
    input: String,
    folder: String,
    name: String,
    qp: u8,
    mode: String,
    busy: tauri::State<'_, Mutex<bool>>,
) -> Result<ResultInfo, String> {
    {
        let mut running = busy.lock().map_err(|_| "Encoder state unavailable.")?;
        if *running {
            return Err("An encode is already running.".into());
        }
        *running = true;
    }
    if mode != "rust" && mode != "c" {
        *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
        return Err("Unknown encoder.".into());
    }
    let ffmpeg = ffmpeg_path(&app);
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let script = if mode == "c" {
        Some(codec_script(&app))
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
    fn names_are_safe_on_all_platforms() {
        assert!(safe_name("My film"));
        for name in ["", "..", "CON", "a/b", "bad.", "a\\b", "x:y"] {
            assert!(!safe_name(name));
        }
    }
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
        let result = encode_pair(Path::new(&ffmpeg), &src, folder, name, 30, None, |p| {
            phases.push(p)
        })
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
        assert!(encode_pair(Path::new(&ffmpeg), &src, folder, name, 30, None, |_| {}).is_err());
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
                |_| {},
            )
            .unwrap();
            let c_video = Temporary(PathBuf::from(&c.video));
            let _c_audio = Temporary(PathBuf::from(&c.audio));
            assert_eq!(c.frames, 24);
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

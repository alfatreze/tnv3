#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tauri::Manager;
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
fn encode_pair(
    ffmpeg: &Path,
    input: &Path,
    folder: &Path,
    name: &str,
    qp: u8,
) -> Result<ResultInfo, String> {
    if qp > 51 || !safe_name(name) {
        return Err("Choose a valid output name and a QP between 0 and 51.".into());
    }
    let video_path = folder.join(format!("{name}.tnv3"));
    let audio_path = folder.join(format!("{name}.mp3"));
    if video_path.exists() || audio_path.exists() {
        return Err("An output file already exists. Choose a different name.".into());
    }
    let (_video_tmp, mut tvc) = temporary_file("tvc")?;
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
    let encoded = tau_video::tvc::encoder::encode_y4m(BufReader::new(stdout), &mut tvc, qp);
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
            tau_video::container::mux_tvc(&mut tvc, &mut video)
                .map_err(|e| format!("Could not create TNV3: {e}"))?;
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
    busy: tauri::State<'_, Mutex<bool>>,
) -> Result<ResultInfo, String> {
    {
        let mut running = busy.lock().map_err(|_| "Encoder state unavailable.")?;
        if *running {
            return Err("An encode is already running.".into());
        }
        *running = true;
    }
    let ffmpeg = ffmpeg_path(&app);
    let result = tauri::async_runtime::spawn_blocking(move || {
        encode_pair(&ffmpeg, Path::new(&input), Path::new(&folder), &name, qp)
    })
    .await;
    *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
    result.map_err(|e| format!("Encoder task stopped: {e}"))?
}
fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(false))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![encode])
        .run(tauri::generate_context!())
        .expect("Failed to start Tau Encoder");
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let result = encode_pair(Path::new(&ffmpeg), &src, folder, name, 30).unwrap();
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
        assert!(encode_pair(Path::new(&ffmpeg), &src, folder, name, 30).is_err());
        assert_eq!(std::fs::read(&audio_guard.0).unwrap(), audio_bytes);
    }
}

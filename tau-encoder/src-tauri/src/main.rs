//! Staging-only Tauri integration draft copied from the read-only production
//! entry point. It is intentionally uncompiled and is not a release candidate.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Integration destination: tau-video exposes this module to the app.
use tau_video::source_gate::SourceGate;
// Integration destination: this helper will live in src-tauri/src.
mod child_supervisor;
mod dock_icon;
mod player;
use serde::{Deserialize, Serialize};
use std::io::BufReader;
#[cfg(feature = "experimental-rust-encoder")]
use std::time::{Duration, Instant};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};
use tauri::{Emitter, Manager};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
struct SourceCheckState(Mutex<Option<Arc<AtomicBool>>>);
#[derive(Default)]
struct SourceDecisionState(Mutex<Option<PendingSourceDecision>>);
struct PendingSourceDecision {
    job_id: String,
    reply: mpsc::SyncSender<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncodePhase {
    Working,
    StopRequested,
    Publishing,
}
struct JobControl {
    phase: Mutex<EncodePhase>,
    stop_requested: AtomicBool,
    source_gate: Mutex<Option<Arc<SourceGate>>>,
    source_stop: Mutex<Option<mpsc::Sender<()>>>,
}
impl Default for JobControl {
    fn default() -> Self {
        Self {
            phase: Mutex::new(EncodePhase::Working),
            stop_requested: AtomicBool::new(false),
            source_gate: Mutex::new(None),
            source_stop: Mutex::new(None),
        }
    }
}
impl JobControl {
    fn request_stop(&self) -> Result<(), String> {
        {
            let mut phase = self.phase.lock().map_err(|_| "Encoder control unavailable.")?;
            match *phase {
            EncodePhase::Working | EncodePhase::StopRequested => {
                *phase = EncodePhase::StopRequested;
            }
            EncodePhase::Publishing => {
                return Err("Saving has started; it is too late to stop this encode.".into());
            }
            }
        }
        self.stop_requested.store(true, Ordering::Release);
        let gate_result = match self.source_gate.lock() {
            Ok(gate) => match gate.as_ref() {
                Some(gate) => gate.stop().map_err(|e| format!("Could not stop source decision gate: {e}")),
                None => Ok(()),
            },
            Err(_) => Err("Source gate state unavailable.".to_owned()),
        };
        let supervisor_result = match self.source_stop.lock() {
            Ok(stop) => {
                if let Some(stop) = stop.as_ref() { let _ = stop.send(()); }
                Ok(())
            }
            Err(_) => Err("Source supervisor state unavailable.".to_owned()),
        };
        match (gate_result, supervisor_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(first), Err(second)) => Err(format!("{first} {second}")),
        }
    }
    fn checkpoint(&self) -> Result<(), String> {
        if self.stop_requested.load(Ordering::Acquire)
            || *self.phase.lock().map_err(|_| "Encoder control unavailable.")? == EncodePhase::StopRequested
        {
            Err("Encoding stopped before output files were created.".into())
        } else {
            Ok(())
        }
    }
    fn begin_publish(&self) -> Result<(), String> {
        let mut phase = self.phase.lock().map_err(|_| "Encoder control unavailable.")?;
        if self.stop_requested.load(Ordering::Acquire) || *phase == EncodePhase::StopRequested {
            return Err("Encoding stopped before output files were created.".into());
        }
        *phase = EncodePhase::Publishing;
        Ok(())
    }
    fn install_source_gate(&self, gate: Arc<SourceGate>) -> Result<(), String> {
        *self.source_gate.lock().map_err(|_| "Source gate state unavailable.")? = Some(gate);
        Ok(())
    }
    fn install_source_stop(&self, stop: mpsc::Sender<()>) -> Result<(), String> {
        let mut current = self.source_stop.lock().map_err(|_| "Source supervisor state unavailable.")?;
        *current = Some(stop.clone());
        if self.stop_requested.load(Ordering::Acquire) {
            let _ = stop.send(());
        }
        Ok(())
    }
    fn source_gate(&self) -> Result<Arc<SourceGate>, String> {
        self.source_gate.lock().map_err(|_| "Source gate state unavailable.")?
            .as_ref().cloned().ok_or("Native source gate is not installed.".into())
    }
    fn is_live_native(&self) -> bool {
        self.source_gate.lock().ok().is_some_and(|gate| gate.is_some())
    }
}
#[derive(Default)]
struct ActiveEncode(Mutex<Option<Arc<JobControl>>>);

#[derive(Serialize, Debug)]
struct SourceCheckResult {
    status: &'static str,
    details: String,
}

#[derive(Clone, Serialize)]
struct SourceErrorDecisionEvent {
    job_id: String,
    details: String,
}
#[derive(Clone, Serialize)]
struct SourceDecisionCancelledEvent {
    job_id: String,
}

fn check_source_impl(
    ffmpeg: &Path,
    input: &Path,
    audio_track: u32,
    cancel: &AtomicBool,
) -> Result<SourceCheckResult, String> {
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(input)
        .args(["-map", "0:v:0", "-map"])
        .arg(format!("0:a:{audio_track}?"))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start source check: {e}"))?;
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Could not capture source-check diagnostics.".into());
        }
    };
    let (sender, receiver) = mpsc::sync_channel::<io::Result<Vec<u8>>>(4);
    let (done_sender, done_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut input = stderr;
        loop {
            let mut buffer = vec![0u8; 2048];
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.truncate(n);
                    if sender.send(Ok(buffer)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = sender.send(Err(e));
                    break;
                }
            }
        }
        let _ = done_sender.send(());
    });
    let mut saved = Vec::new();
    let mut stopped = false;
    let mut issue_detected = false;
    let mut read_error = None;
    let mut exited_at = None;
    loop {
        if cancel.load(Ordering::Acquire) {
            stopped = true;
            break;
        }
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Ok(bytes)) => {
                let available = 4096usize.saturating_sub(saved.len());
                saved.extend_from_slice(&bytes[..bytes.len().min(available)]);
                let text = String::from_utf8_lossy(&saved).to_lowercase();
                if (text.contains("[matroska,webm")
                    && (text.contains("invalid as first byte")
                        || text.contains("duplicate element")
                        || text.contains("exceeds containing master")))
                    || text.contains("could not find ref with poc")
                {
                    issue_detected = true;
                    break;
                }
            }
            Ok(Err(e)) => {
                read_error = Some(e.to_string());
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => match child.try_wait() {
                Ok(Some(_)) => {
                    let since = exited_at.get_or_insert_with(std::time::Instant::now);
                    if since.elapsed() >= std::time::Duration::from_secs(1) {
                        read_error = Some("the source-check diagnostic pipe did not close".into());
                        break;
                    }
                    continue;
                }
                Ok(None) => continue,
                Err(e) => {
                    read_error = Some(e.to_string());
                    break;
                }
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let finished = match child.try_wait() {
        Ok(status) => status,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            drop(receiver);
            let _ = done_receiver.recv_timeout(std::time::Duration::from_secs(1));
            return Err(format!("Could not inspect source-check process: {e}"));
        }
    };
    let exited_before_stop = finished.is_some();
    let status = match finished {
        Some(status) => status,
        None if stopped || issue_detected || read_error.is_some() => {
            if let Err(error) = child.kill() {
                if child.try_wait().ok().flatten().is_none() {
                    drop(receiver);
                    return Err(format!(
                        "Could not stop the source-check process; cleanup is unverified: {error}"
                    ));
                }
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("Could not wait for the source check: {e}"))?
                {
                    break status;
                }
                if std::time::Instant::now() >= deadline {
                    drop(receiver);
                    return Err("Source-check process did not exit; cleanup is unverified.".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        None => {
            let mut deadline = None;
            loop {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("Could not wait for source check: {e}"))?
                {
                    break status;
                }
                if cancel.load(Ordering::Acquire) && deadline.is_none() {
                    stopped = true;
                    child.kill().map_err(|e| {
                        format!(
                            "Could not stop the source-check process; cleanup is unverified: {e}"
                        )
                    })?;
                    deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                if deadline.is_some_and(|until| std::time::Instant::now() >= until) {
                    drop(receiver);
                    return Err("Source-check process did not exit; cleanup is unverified.".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    };
    drop(receiver);
    if done_receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .is_err()
    {
        return Err(
            "Source-check process exited, but diagnostic pipe cleanup could not be verified."
                .into(),
        );
    }
    reader
        .join()
        .map_err(|_| "Could not collect source-check diagnostics.")?;
    if let Some(error) = read_error {
        return Err(format!("Could not read source-check diagnostics: {error}"));
    }
    if stopped || cancel.load(Ordering::Acquire) {
        return Ok(SourceCheckResult {
            status: "stopped",
            details: String::new(),
        });
    }
    let details = String::from_utf8_lossy(&saved).trim().to_owned();
    if !status.success() && (exited_before_stop || !issue_detected) {
        return Err(with_diagnostics("Source check failed.".into(), &details));
    }
    Ok(SourceCheckResult {
        status: if issue_detected || !details.is_empty() {
            "warning"
        } else {
            "clean"
        },
        details,
    })
}
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

/// Reject occupied destinations and invalid folders before probing or encoding.
/// This metadata-only check does not prove writability; final output creation
/// later still uses `create_new`, which closes the race before publication.
fn preflight_output_directory(
    folder: &Path,
    video_path: &Path,
    audio_path: &Path,
) -> Result<(), String> {
    for path in [video_path, audio_path] {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err("An output file already exists. Choose a different name.".into());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not inspect output path: {error}")),
        }
    }
    if !folder.is_dir() {
        return Err("Choose an existing output folder.".into());
    }
    Ok(())
}
#[derive(Serialize, Debug)]
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
    stderr: impl Read + Send + 'static,
) -> std::thread::JoinHandle<io::Result<String>> {
    capture_diagnostics_with_limit(stderr, 4096)
}
fn capture_diagnostics_with_limit(
    mut stderr: impl Read + Send + 'static,
    limit: usize,
) -> std::thread::JoinHandle<io::Result<String>> {
    std::thread::spawn(move || {
        let mut saved = Vec::new();
        let mut buf = [0u8; 4096];
        let mut truncated = false;
        loop {
            let n = stderr.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let take = n.min(limit.saturating_sub(saved.len()));
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
fn join_with_timeout<T>(
    handle: std::thread::JoinHandle<T>,
    label: &str,
) -> Result<T, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !handle.is_finished() {
        if std::time::Instant::now() >= deadline {
            drop(handle);
            return Err(format!("{label} did not close; cleanup is unverified."));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    handle.join().map_err(|_| format!("{label} reader panicked."))
}
fn finish_diagnostics(
    handle: std::thread::JoinHandle<io::Result<String>>,
) -> Result<String, String> {
    handle
        .join()
        .map_err(|_| "Could not collect decoder diagnostics.".to_owned())?
        .map_err(|e| format!("Could not read decoder diagnostics: {e}"))
}
fn finish_live_diagnostics(
    result: io::Result<()>,
    control: &JobControl,
    label: &str,
) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(error)
            if control.stop_requested.load(Ordering::Acquire)
                && error.to_string() == "encoding stopped" => control.checkpoint(),
        Err(error) => Err(format!("Could not drain {label} diagnostics: {error}")),
    }
}
fn with_diagnostics(message: String, details: &str) -> String {
    if details.is_empty() {
        message
    } else {
        format!("{message}\n{details}")
    }
}

/// Shows the encode progress on the app's Dock icon (macOS), the taskbar button (Windows) or the launcher
/// (Linux); `None` clears it. Video counts for 0..90 %, then audio 92 %, saving 98 %; probing and unknown
/// totals show as indeterminate.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn show_system_progress(app: &tauri::AppHandle, progress: Option<&Progress>) {
    use tauri::window::{ProgressBarState, ProgressBarStatus};
    let Some(window) = app.get_webview_window("main") else { return };
    let state = match progress {
        None => ProgressBarState { status: Some(ProgressBarStatus::None), progress: None },
        Some(p) => match system_progress_percent(p) {
            Some(percent) => ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some(percent) },
            None => ProgressBarState { status: Some(ProgressBarStatus::Indeterminate), progress: None },
        },
    };
    let _ = window.set_progress_bar(state);
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn system_progress_percent(p: &Progress) -> Option<u64> {
    match (p.phase, p.total_frames_estimate) {
        ("video" | "c-video", Some(total)) if total > 0 => {
            Some((f64::from(p.frames.min(total)) / f64::from(total) * 90.0) as u64)
        }
        ("audio", _) => Some(92),
        ("saving", _) => Some(98),
        _ => None,
    }
}

/// What the animated Dock icon shows for a progress event (video fraction, audio and subtitles shimmer).
fn icon_phase(p: &Progress) -> dock_icon::IconPhase {
    use dock_icon::IconPhase;
    match p.phase {
        "video" | "c-video" => IconPhase::Video(p.total_frames_estimate.filter(|&t| t > 0).map(|t| p.frames.min(t) as f32 / t as f32)),
        "audio" => IconPhase::Audio,
        "subtitles" => IconPhase::Subtitles,
        "saving" => IconPhase::Saving,
        _ => IconPhase::Probing,
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
    let stderr = child.stderr.take()?;
    let buf = read_bounded_and_drain(stderr, 65536).ok()?;
    let _ = child.wait();
    let seconds = parse_duration(&String::from_utf8_lossy(&buf))?;
    Some((seconds * 24.0).ceil() as u32)
}

fn read_bounded_and_drain(mut input: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut saved = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 4096];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(saved);
        }
        let take = count.min(limit.saturating_sub(saved.len()));
        saved.extend_from_slice(&buffer[..take]);
        // Continue reading after the retained excerpt is full to avoid closing
        // the pipe early and blocking the child on stderr.
    }
}

fn estimated_frames_native(ffmpeg: &Path, input: &Path, control: &JobControl) -> Result<Option<u32>, String> {
    control.checkpoint()?;
    let mut command = Command::new(ffmpeg);
    command.args(["-nostdin", "-hide_banner", "-i"]).arg(input);
    let child = child_supervisor::spawn_source(&mut command)
        .map_err(|e| format!("Could not start supervised duration probe: {e}"))?;
    if let Err(error) = control.install_source_stop(child.stop.clone()) {
        let _ = child.stop.send(());
        return match child.finished.join() {
            Ok(Ok(_)) => Err(error),
            _ => Err(format!("{error}; duration probe cleanup is unverified")),
        };
    }
    let log = capture_diagnostics_with_limit(child.stderr, 65536);
    let status = match child.finished.join() {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return Err(format!("Duration probe cleanup is unverified: {error}")),
        Err(_) => return Err("Duration probe supervisor panicked; cleanup is unverified.".into()),
    };
    let details = join_with_timeout(log, "Duration probe diagnostic pipe")?
        .map_err(|e| format!("Could not read duration probe diagnostics: {e}"))?;
    let _ = status; // FFmpeg's metadata-only invocation normally exits nonzero.
    control.checkpoint()?;
    Ok(parse_duration(&details).map(|seconds| (seconds * 24.0).ceil() as u32))
}

/// Audio stored with the video. TAC is embedded in the .tnv3 (default); the MP3 sidecar is the
/// parked legacy pairing and is only used when asked for explicitly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AudioFormat {
    #[default]
    Tac,
    Mp3,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AdvancedSettings {
    audio: AudioFormat,
    round_down: bool,
    eof_pass: bool,
    trim_mp3_delay: bool,
    width: Option<u32>,
    height: Option<u32>,
    level: Option<u8>,
    /// Zero-based index among the source's audio streams; `None` is the first.
    audio_track: Option<u32>,
    /// Zero-based index among the source's text subtitle streams to author into the file; `None` = none.
    subtitle_track: Option<u32>,
}
impl AdvancedSettings {
    fn track(self) -> u32 {
        self.audio_track.unwrap_or(0)
    }
    fn dimensions(self) -> (u32, u32) {
        (self.width.unwrap_or(400), self.height.unwrap_or(360))
    }
    fn level(self) -> u8 {
        self.level.unwrap_or(0)
    }
    fn fps_filter(self) -> &'static str {
        match (self.round_down, self.eof_pass) {
            (false, _) => "fps=24",
            (true, false) => "fps=24:round=down",
            (true, true) => "fps=24:round=down:eof_action=pass",
        }
    }
    fn audio_filter(self) -> &'static str {
        if self.audio == AudioFormat::Mp3 && self.trim_mp3_delay {
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
    tac_tool: String,
    /// TrueType font for drawn subtitles; required only when subtitles are selected.
    subtitle_font: String,
    workspace_limit_bytes: u64,
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
    progress: impl FnMut(Progress),
) -> Result<ResultInfo, String> {
    let control = JobControl::default();
    // This convenience wrapper has no UI decision channel; preserve its old
    // auto-continue behaviour for direct native callers. The Tauri command
    // installs its job-scoped live gate before entering encode_pair_impl.
    if c_tools.is_none() {
        let (events, _pending) = mpsc::channel();
        control.install_source_gate(Arc::new(SourceGate::new(true, events)))?;
    }
    encode_pair_impl(
        ffmpeg,
        input,
        folder,
        name,
        qp,
        c_tools,
        false,
        advanced,
        dependencies,
        &control,
        |_| Ok(()),
        progress,
    )
}

fn encode_pair_impl(
    ffmpeg: &Path,
    input: &Path,
    folder: &Path,
    name: &str,
    qp: u8,
    c_tools: Option<(&Path, &Path)>,
    pure_rust: bool,
    advanced: AdvancedSettings,
    dependencies: &Dependencies,
    control: &JobControl,
    mut confirm_source: impl FnMut(&str) -> Result<(), String>,
    mut progress: impl FnMut(Progress),
) -> Result<ResultInfo, String> {
    if qp > 51 || !safe_name(name) {
        return Err("Choose a valid output name and a QP between 0 and 51.".into());
    }
    let (width, height) = advanced.dimensions();
    if !(2..=800).contains(&width)
        || !(2..=720).contains(&height)
        || width % 2 != 0
        || height % 2 != 0
        || advanced.level() > 3
    {
        return Err(
            "Choose even output dimensions up to 800×720 and a level from 1 to 3, or automatic."
                .into(),
        );
    }
    if advanced.track() > 127 || advanced.subtitle_track.is_some_and(|t| t > 127) {
        return Err("Choose an audio track and a subtitle track from 0 to 127.".into());
    }
    if advanced.subtitle_track.is_some() && advanced.audio != AudioFormat::Tac {
        return Err("Subtitles need the embedded TAC output.".into());
    }
    if pure_rust && advanced.level() != 0 {
        return Err(
            "Manual level selection is not supported by the experimental Rust encoder.".into(),
        );
    }
    let video_path = folder.join(format!("{name}.tnv3"));
    let tac_audio = advanced.audio == AudioFormat::Tac;
    let audio_path = if tac_audio {
        video_path.clone()
    } else {
        folder.join(format!("{name}.mp3"))
    };
    preflight_output_directory(folder, &video_path, &audio_path)?;
    control.checkpoint()?;
    progress(Progress {
        phase: "probing",
        frames: 0,
        total_frames_estimate: None,
        remaining_seconds: None,
    });
    let total = if control.is_live_native() {
        estimated_frames_native(ffmpeg, input, control)?
    } else {
        estimated_frames(ffmpeg, input)
    };
    progress(Progress {
        phase: "video",
        frames: 0,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    let mut warnings = Vec::new();
    let (video_tmp, mut tvc) = temporary_file("tnv3")?;
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
        let report = tau_video::c_encoder::encode_video_with_geometry(
            &tools,
            input,
            &mut tvc,
            tau_video::c_encoder::Options { qp, jobs, timing },
            width,
            height,
            advanced.level(),
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
            control.checkpoint()?;
            confirm_source(&report.diagnostics)?;
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
        if pure_rust {
            #[cfg(not(feature = "experimental-rust-encoder"))]
            {
                return Err("The experimental Rust encoder is not enabled in this build.".into());
            }
            #[cfg(feature = "experimental-rust-encoder")]
            {
                let video_filter = format!(
                    "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2,{}",
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
                        if frames != 1
                            && now.duration_since(last_update) < Duration::from_millis(500)
                        {
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
                let stats = encoded.map_err(|e| {
                    with_diagnostics(format!("Video encoding failed: {e}"), &details)
                })?;
                if !status.success() {
                    return Err(with_diagnostics(
                        process_failed(status, "Video conversion"),
                        &details,
                    ));
                }
                if !details.is_empty() {
                    control.checkpoint()?;
                    confirm_source(&details)?;
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
        } else {
                // Native integration point (draft contract): replace this
                // decoder's direct spawn with child_supervisor::spawn_source,
                // drain its stderr through SourceGate::report, and pass the
                // gate-wrapped Y4M reader to
                // staged_native_orchestrator::encode_y4m_video_with_gate.
                // Stop must set the shared AtomicBool, call gate.stop(), and
                // signal the supervisor before waiting, so a blocked read is
                // released. Join the diagnostic drainer, reap/check decoder
                // status, then gate.checkpoint() before audio/publication.
                // The Rust+C orchestrator must itself own and reap every C
                // worker, mux/check helper, and cancellation failure; its
                // current staged API does not yet enforce process-tree safety.
            let tvc_encoder = selected_file(&dependencies.tvc_encoder, "TVC encoder")?
                .ok_or("The Rust C workflow requires tvc_enc; set it in Dependency settings or bundle it with the app.")?;
            let tnv3_tool = selected_file(&dependencies.tnv3_tool, "TNV3 tool")?
                .ok_or("The Rust C workflow requires tnv3; set it in Dependency settings or bundle it with the app.")?;
            let video_filter = format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2,{}",
                advanced.fps_filter()
            );
            let mut decoder_command = Command::new(ffmpeg);
            decoder_command
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
                .stderr(Stdio::piped());
            let gate = control.source_gate()?;
            let decoder = child_supervisor::spawn_source(&mut decoder_command)
                .map_err(|e| format!("Could not start supervised FFmpeg: {e}"))?;
            if let Err(error) = control.install_source_stop(decoder.stop.clone()) {
                let _ = decoder.stop.send(());
                return match decoder.finished.join() {
                    Ok(Ok(_)) => Err(error),
                    Ok(Err(cleanup)) => Err(format!("{error}; decoder cleanup is unverified: {cleanup}")),
                    Err(_) => Err(format!("{error}; decoder supervisor panicked, cleanup is unverified")),
                };
            }
            let log = tau_video::source_gate::drain_diagnostics(decoder.stderr, gate.clone());
            let stdout = decoder.stdout;
            // Four parallel segments fit the default 512 MiB budget (checked with the CLI at 400 and 800 wide: peak
            // 150 MB resident, no budget error) and cut the wall time of a film by about a quarter against two.
            let jobs = std::thread::available_parallelism().map_or(1, |n| n.get().clamp(1, 4));
            let workspace_limit_bytes = if dependencies.workspace_limit_bytes == 0 {
                512u64 * 1024 * 1024
            } else {
                dependencies.workspace_limit_bytes
            };
            let video_started = std::time::Instant::now();
            let encoded = tau_video::experimental_c_orchestrator::encode_y4m_video_with_gate(
                &tau_video::experimental_c_orchestrator::Tools {
                    tvc_encoder,
                    tnv3: tnv3_tool,
                },
                BufReader::new(stdout),
                &mut tvc,
                tau_video::experimental_c_orchestrator::Options {
                    qp,
                    jobs,
                    frames_per_segment: (480u32 * 400 * 360 / (width * height)).clamp(1, 480)
                        as usize,
                    level: advanced.level(),
                    workspace_limit_bytes,
                    ..Default::default()
                },
                Some(&control.stop_requested),
                gate.clone(),
                |frames| {
                    let frames = u32::try_from(frames).unwrap_or(u32::MAX);
                    let seconds = video_started.elapsed().as_secs_f64();
                    let remaining_seconds = total
                        .filter(|&n| frames < n && seconds >= 2.0 && frames > 0)
                        .map(|n| {
                            ((f64::from(n - frames) * seconds) / f64::from(frames)).ceil() as u64
                        });
                    progress(Progress {
                        phase: "video",
                        frames,
                        total_frames_estimate: total,
                        remaining_seconds,
                    });
                },
            );
            if encoded.is_err() {
                let _ = decoder.stop.send(());
            }
            let status = match decoder.finished.join() {
                Ok(Ok(status)) => status,
                Ok(Err(error)) => return Err(format!("Could not reap supervised FFmpeg; cleanup is unverified: {error}")),
                Err(_) => return Err("Source supervisor panicked; decoder cleanup is unverified.".into()),
            };
            let log_result = match join_with_timeout(log, "Source diagnostic pipe") {
                Ok(result) => result,
                Err(error) => {
                    let _ = gate.stop();
                    let _ = control.request_stop();
                    return Err(match &encoded {
                        Err(encode_error) => format!("{error}; video encode also failed: {encode_error}"),
                        Ok(_) => error,
                    });
                }
            };
            if let Err(drain_error) = finish_live_diagnostics(log_result, control, "source") {
                if let Err(encode_error) = &encoded {
                    if control.stop_requested.load(Ordering::Acquire) {
                        return Err(format!("Encoding stopped before output files were created: {encode_error}; diagnostic cleanup also failed: {drain_error}"));
                    }
                }
                return Err(drain_error);
            }
            let (details, diagnostics_truncated) = gate.take_diagnostics()
                .map_err(|e| format!("Could not collect source diagnostics: {e}"))?;
            let details = if diagnostics_truncated {
                format!("{details}\n[Further diagnostics omitted.]")
            } else {
                details
            };
            let report = match encoded {
                Ok(report) => report,
                Err(error) => {
                    if control.stop_requested.load(Ordering::Acquire) {
                        return Err(with_diagnostics(format!("Encoding stopped before output files were created: {error}"), &details));
                    }
                    let _ = gate.stop();
                    let _ = control.request_stop();
                    return Err(with_diagnostics(format!("Video encoding failed: {error}"), &details));
                }
            };
            control.checkpoint()?;
            gate.checkpoint().map_err(|e| format!("Source decision failed: {e}"))?;
            if !status.success() {
                return Err(with_diagnostics(
                    process_failed(status, "Video conversion"),
                    &details,
                ));
            }
            if !details.is_empty() {
                control.checkpoint()?;
                confirm_source(&details)?;
                warnings.push(with_diagnostics(
                    "Source decoding reported errors; inspect the output.".into(),
                    &details,
                ));
            }
            progress(Progress {
                phase: "video",
                frames: report.frames,
                total_frames_estimate: total,
                remaining_seconds: None,
            });
            VideoSummary {
                frames: report.frames,
                level: report.level,
                reencoded: report.joint_reencodes,
            }
        }
    };
    control.checkpoint()?;
    progress(Progress {
        phase: "audio",
        frames: stats.frames,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    control.checkpoint()?;
    let (audio_tmp, audio_file) = temporary_file(if tac_audio { "wav" } else { "mp3" })?;
    drop(audio_file);
    let duration = format!("{:.9}", f64::from(stats.frames) / 24.0);
    let mut audio_command = Command::new(ffmpeg);
    audio_command
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-map",
            &format!("0:a:{}", advanced.track()),
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
        ])
        .args(if tac_audio {
            ["-c:a", "pcm_s16le", "-f", "wav"].as_slice()
        } else {
            ["-c:a", "libmp3lame", "-b:a", "128k", "-f", "mp3"].as_slice()
        })
        .arg(&audio_tmp.0)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let (status, details) = if control.is_live_native() {
        let gate = control.source_gate()?;
        let child = child_supervisor::spawn_source(&mut audio_command)
            .map_err(|e| format!("Could not start supervised audio FFmpeg: {e}"))?;
        if let Err(error) = control.install_source_stop(child.stop.clone()) {
            let _ = child.stop.send(());
            return match child.finished.join() {
                Ok(Ok(_)) => Err(error),
                Ok(Err(cleanup)) => {
                    let audio_path = audio_tmp.0.clone();
                    std::mem::forget(audio_tmp);
                    Err(format!("{error}; audio child cleanup is unverified ({cleanup}); temporary output retained at {}", audio_path.display()))
                }
                Err(_) => {
                    let audio_path = audio_tmp.0.clone();
                    std::mem::forget(audio_tmp);
                    Err(format!("{error}; audio supervisor panicked, cleanup is unverified; temporary output retained at {}", audio_path.display()))
                }
            };
        }
        let log = tau_video::source_gate::drain_diagnostics(child.stderr, gate.clone());
        let status = match child.finished.join() {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => {
                let audio_path = audio_tmp.0.clone();
                std::mem::forget(audio_tmp);
                return Err(format!("Could not reap audio FFmpeg; cleanup is unverified: {error}; temporary output retained at {}", audio_path.display()));
            }
            Err(_) => {
                let audio_path = audio_tmp.0.clone();
                std::mem::forget(audio_tmp);
                return Err(format!("Audio source supervisor panicked; cleanup is unverified; temporary output retained at {}", audio_path.display()));
            }
        };
        let log_result = match join_with_timeout(log, "Audio diagnostic pipe") {
            Ok(result) => result,
            Err(error) => {
                let audio_path = audio_tmp.0.clone();
                std::mem::forget(audio_tmp);
                let _ = gate.stop();
                let _ = control.request_stop();
                return Err(format!("{error}; audio temporary output retained at {}", audio_path.display()));
            }
        };
        finish_live_diagnostics(log_result, control, "audio")?;
        control.checkpoint()?;
        gate.checkpoint().map_err(|e| format!("Source decision failed: {e}"))?;
        let (details, diagnostics_truncated) = gate.take_diagnostics()
            .map_err(|e| format!("Could not collect audio diagnostics: {e}"))?;
        let details = if diagnostics_truncated {
            format!("{details}\n[Further diagnostics omitted.]")
        } else {
            details
        };
        (status, details)
    } else {
        let mut child = audio_command.spawn()
            .map_err(|e| format!("Could not start FFmpeg: {e}"))?;
        let stderr = child.stderr.take().ok_or("Could not capture audio diagnostics.")?;
        let log = capture_diagnostics(stderr);
        let status = child.wait().map_err(|e| format!("Could not wait for FFmpeg: {e}"))?;
        (status, finish_diagnostics(log)?)
    };
    if !status.success() {
        return Err(with_diagnostics(
            process_failed(status, if tac_audio { "Audio conversion" } else { "MP3 encoding" }),
            &details,
        ));
    }
    if !details.is_empty() {
        control.checkpoint()?;
        confirm_source(&details)?;
        warnings.push(with_diagnostics(
            "Audio decoding reported errors; inspect the output.".into(),
            &details,
        ));
    }
    let tac_result = if tac_audio {
        let mut fallbacks = Vec::new();
        if let Some((script, _)) = c_tools {
            if let Some(dir) = script.parent() {
                fallbacks.push(dir.join(executable_name("tnv3")));
                fallbacks.push(dir.join("..").join("tac_ref").join(executable_name("tac")));
            }
        }
        let (guard, file, sub_warnings) = build_tnv3_with_tac(
            &mut tvc,
            &video_tmp.0,
            &audio_tmp.0,
            pure_rust,
            dependencies,
            &fallbacks,
            control,
            input,
            ffmpeg,
            advanced.subtitle_track,
            &mut |phase| {
                progress(Progress { phase, frames: stats.frames, total_frames_estimate: total, remaining_seconds: None })
            },
        )?;
        warnings.extend(sub_warnings);
        Some((guard, file))
    } else {
        None
    };
    control.begin_publish()?;
    progress(Progress {
        phase: "saving",
        frames: stats.frames,
        total_frames_estimate: total,
        remaining_seconds: None,
    });
    tvc.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Could not read temporary video: {e}"))?;
    if let Some((final_tmp, _)) = tac_result {
        let mut video = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&video_path)
            .map_err(|e| format!("Could not create video output: {e}"))?;
        let result = (|| {
            io::copy(
                &mut File::open(&final_tmp.0).map_err(|e| e.to_string())?,
                &mut video,
            )
            .map_err(|e| format!("Could not copy TNV3: {e}"))?;
            video
                .sync_all()
                .map_err(|e| format!("Could not finish output file: {e}"))
        })();
        drop(video);
        if let Err(e) = result {
            return match std::fs::remove_file(&video_path) {
                Ok(()) => Err(e),
                Err(cleanup) => Err(format!("{e} Could not remove incomplete video: {cleanup}")),
            };
        }
        return Ok(ResultInfo {
            frames: stats.frames,
            level: stats.level,
            reencoded: stats.reencoded,
            video: video_path.display().to_string(),
            audio: String::new(),
            warnings,
        });
    }
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
            if pure_rust {
                tau_video::container::mux_tvc(&mut tvc, &mut video)
                    .map_err(|e| format!("Could not create TNV3: {e}"))?;
            } else {
                io::copy(&mut tvc, &mut video).map_err(|e| format!("Could not copy TNV3: {e}"))?;
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
/// Finds a tool: the configured path, else the first existing fallback.
fn resolve_tool(configured: &str, label: &str, fallbacks: &[PathBuf]) -> Result<PathBuf, String> {
    if let Some(path) = selected_file(configured, label)? {
        return Ok(path);
    }
    fallbacks
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| format!("{label} is required for TAC audio; set it in Dependency settings or bundle it with the app."))
}

/// Runs a short-lived child, polling so Stop can kill it. Returns its captured stderr.
fn run_cancellable(mut command: Command, label: &str, control: &JobControl) -> Result<String, String> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("Could not start {label}: {e}"))?;
    let stderr = child.stderr.take().ok_or("Could not capture tool diagnostics.")?;
    let log = capture_diagnostics(stderr);
    let status = loop {
        if let Err(stop) = control.checkpoint() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = finish_diagnostics(log);
            return Err(stop);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Could not wait for {label}: {e}"));
            }
        }
    };
    let details = finish_diagnostics(log)?;
    if !status.success() {
        return Err(with_diagnostics(process_failed(status, label), &details));
    }
    Ok(details)
}

/// Turns the finished video-only stream plus the WAV into one TNV3 with embedded TAC audio:
/// (pure-Rust only: wrap the TVC in TNV3), `tnv3 demux` to TVC, `tac enc`, `tnv3 mux -a`.
/// Returns the temporary result (guard and file).
fn build_tnv3_with_tac(
    tvc: &mut File,
    video_path: &Path,
    wav_path: &Path,
    pure_rust: bool,
    dependencies: &Dependencies,
    fallbacks: &[PathBuf],
    control: &JobControl,
    input: &Path,
    ffmpeg: &Path,
    subtitle_track: Option<u32>,
    on_phase: &mut dyn FnMut(&'static str),
) -> Result<(Temporary, File, Vec<String>), String> {
    let tac = resolve_tool(&dependencies.tac_tool, "TAC encoder", fallbacks)?;
    control.checkpoint()?;
    let mut video_source = video_path.to_path_buf();
    let mut _wrapped = None;
    if pure_rust {
        let (guard, mut file) = temporary_file("tnv3")?;
        tvc.seek(SeekFrom::Start(0)).map_err(|e| format!("Could not read temporary video: {e}"))?;
        tau_video::container::mux_tvc(tvc, &mut file).map_err(|e| format!("Could not create TNV3: {e}"))?;
        file.sync_all().map_err(|e| format!("Could not write temporary TNV3: {e}"))?;
        video_source = guard.0.clone();
        _wrapped = Some(guard);
    }
    let mut header = [0u8; 64];
    File::open(&video_source)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| format!("Could not read the temporary video header: {e}"))?;
    let crc = header[6] & 1 != 0;
    let word = |o: usize| u32::from(u16::from_le_bytes([header[o], header[o + 1]]));
    let long = |o: usize| u32::from_le_bytes([header[o], header[o + 1], header[o + 2], header[o + 3]]);
    let (picture_w, picture_h, fps_n, fps_d) = (word(8), word(10), long(12), long(16));
    let (tac_guard, tac_file) = temporary_file("tac")?;
    drop(tac_file);
    let tvc_bytes = tau_video::container::extract_tvc(
        File::open(&video_source).map_err(|e| format!("Could not read the temporary video: {e}"))?,
    )
    .map_err(|e| format!("Could not extract the video stream: {e}"))?;
    control.checkpoint()?;
    let mut enc = Command::new(&tac);
    enc.arg("enc").arg(wav_path).arg(&tac_guard.0).arg("-b").arg("96");
    run_cancellable(enc, "TAC encoding", control)?;
    let mut warnings = Vec::new();
    let (area_guard, area_file) = temporary_file("bin")?;
    drop(area_file);
    if let Some(track) = subtitle_track {
        on_phase("subtitles");
        // Font: the one chosen in settings, else TAU_SUBTITLE_FONT, else the built-in Roboto.
        let fonts = match selected_file(&dependencies.subtitle_font, "Subtitle font")?
            .or_else(|| std::env::var_os("TAU_SUBTITLE_FONT").map(PathBuf::from).filter(|p| p.is_file()))
        {
            Some(path) => vec![tau_video::subtitle::FontSpec::from_argument(&path.display().to_string())
                .map_err(|e| e.to_string())?],
            None => tau_video::subtitle::default_fonts(),
        };
        let (srt_guard, srt_file) = temporary_file("ass")?;
        drop(srt_file);
        let mut extract = Command::new(ffmpeg);
        extract
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(input)
            .arg("-map")
            .arg(format!("0:s:{track}"))
            .args(["-c:s", "ass", "-f", "ass"])
            .arg(&srt_guard.0);
        run_cancellable(extract, "Subtitle extraction (only text subtitles can be converted)", control)?;
        control.checkpoint()?;
        let text = std::fs::read_to_string(&srt_guard.0)
            .map_err(|e| format!("Could not read the extracted subtitles: {e}"))?;
        let mut options = tau_video::subtitle::Options::new(
            vec![tau_video::subtitle::TrackInput {
                name: "subtitles.ass".into(),
                text,
                language: "und".into(),
                default: true,
                forced: false,
                quiet_font_fallback: true,
            }],
            fonts,
            (picture_w, picture_h),
        );
        options.fps = Some((fps_n, fps_d));
        options.cells = vec![(16, 24), (20, 28), (26, 36)];
        let (area, info) = tau_video::subtitle::build_area(&options)
            .map_err(|e| format!("Subtitle authoring failed: {e}"))?;
        std::fs::write(&area_guard.0, &area).map_err(|e| format!("Could not write the subtitle area: {e}"))?;
        warnings.extend(info.warnings.iter().map(|w| format!("Subtitles: {w}")));
    }
    let audio_bytes = std::fs::read(&tac_guard.0).map_err(|e| format!("Could not read the encoded audio: {e}"))?;
    let area_bytes = match subtitle_track {
        Some(_) => Some(std::fs::read(&area_guard.0).map_err(|e| format!("Could not read the subtitle area: {e}"))?),
        None => None,
    };
    let mut muxed = std::io::Cursor::new(Vec::new());
    tau_video::container::mux_streams(
        &tvc_bytes,
        &tau_video::container::MuxOptions {
            audio: Some(&audio_bytes),
            subtitles: area_bytes.as_deref(),
            crc,
            trim_audio: true,
            ..Default::default()
        },
        &mut muxed,
    )
    .map_err(|e| format!("Muxing audio and subtitles failed: {e}"))?;
    let (out_guard, mut out_file) = temporary_file("tnv3")?;
    std::io::Write::write_all(&mut out_file, muxed.get_ref())
        .and_then(|_| out_file.sync_all())
        .map_err(|e| format!("Could not write the muxed TNV3: {e}"))?;
    drop(out_file);
    let file = File::open(&out_guard.0).map_err(|e| format!("Could not open muxed TNV3: {e}"))?;
    Ok((out_guard, file, warnings))
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

#[derive(Serialize)]
struct ToolDiscovery {
    key: &'static str,
    automatic: String,
    candidates: Vec<String>,
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn resolve_program(path: PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path);
    }
    if path.components().count() != 1 {
        return None;
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join(&path))
        .find(|candidate| candidate.is_file())
}

fn discovery_entry(
    key: &'static str,
    automatic: Option<PathBuf>,
    extras: Vec<PathBuf>,
    names: &[&str],
) -> ToolDiscovery {
    let automatic = automatic.and_then(resolve_program);
    let mut paths = Vec::new();
    let mut add = |path: PathBuf| {
        if path.is_file() {
            let path = path.to_string_lossy().into_owned();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    };
    if let Some(path) = &automatic {
        add(path.clone());
    }
    for path in extras {
        add(path);
    }
    for dir in std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
    {
        for name in names {
            add(dir.join(executable_name(name)));
        }
    }
    #[cfg(target_os = "macos")]
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        for name in names {
            add(Path::new(dir).join(executable_name(name)));
        }
    }
    ToolDiscovery {
        key,
        automatic: automatic
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        candidates: paths,
    }
}

#[tauri::command]
fn discover_tools(
    app: tauri::AppHandle,
    dependencies: Dependencies,
    workflow: String,
) -> Vec<ToolDiscovery> {
    let resources = app.path().resource_dir().ok();
    let bundled = |path: &str| resources.as_ref().map(|dir| dir.join(path));
    let ffmpeg_auto = ffmpeg_path(&app);
    let ffmpeg_used = if dependencies.ffmpeg.trim().is_empty() {
        ffmpeg_auto.clone()
    } else {
        PathBuf::from(&dependencies.ffmpeg)
    };
    let ffprobe_auto = ffmpeg_used.with_file_name(executable_name("ffprobe"));
    let script_auto = codec_script(&app);
    let script_used = if dependencies.script.trim().is_empty() {
        script_auto.clone()
    } else {
        PathBuf::from(&dependencies.script)
    };
    let python_auto = std::env::var_os("TAU_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(if cfg!(windows) {
                "python.exe"
            } else {
                "python3"
            })
        });
    let tool_auto = |env: &str, bundled_path: &str, script_relative: &str| {
        if workflow == "native" {
            bundled(bundled_path)
        } else {
            std::env::var_os(env)
                .map(PathBuf::from)
                .or_else(|| script_used.parent().map(|p| p.join(script_relative)))
        }
    };
    let env_path = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>()
    };
    let mut ffmpeg_extra = env_path("TAU_FFMPEG");
    if let Some(p) = bundled(&format!("ffmpeg/{}", executable_name("ffmpeg"))) {
        ffmpeg_extra.push(p);
    }
    let mut ffprobe_extra = Vec::new();
    if let Some(p) = bundled(&format!("ffmpeg/{}", executable_name("ffprobe"))) {
        ffprobe_extra.push(p);
    }
    let tvc_name = executable_name("tvc_enc");
    let tnv3_name = executable_name("tnv3");
    let tvc_bundle = format!("codec-tools/tvc_ref/{tvc_name}");
    let tnv3_bundle = format!("codec-tools/tnv3_ref/{tnv3_name}");
    let mut tvc_extra = env_path("TAU_TVC_ENC");
    tvc_extra.extend(bundled(&tvc_bundle));
    let mut tnv3_extra = env_path("TAU_TNV3");
    tnv3_extra.extend(bundled(&tnv3_bundle));
    let tac_name = executable_name("tac");
    let tac_bundle = format!("codec-tools/tac_ref/{tac_name}");
    let mut tac_extra = env_path("TAU_TAC");
    tac_extra.extend(bundled(&tac_bundle));
    vec![
        discovery_entry("ffmpeg", Some(ffmpeg_auto), ffmpeg_extra, &["ffmpeg"]),
        discovery_entry("ffprobe", Some(ffprobe_auto), ffprobe_extra, &["ffprobe"]),
        discovery_entry(
            "python",
            Some(python_auto),
            env_path("TAU_PYTHON"),
            &["python3", "python"],
        ),
        discovery_entry("script", Some(script_auto), Vec::new(), &[]),
        discovery_entry(
            "tvcEncoder",
            tool_auto(
                "TAU_TVC_ENC",
                &tvc_bundle,
                &format!("../tvc_ref/{tvc_name}"),
            ),
            tvc_extra,
            &[],
        ),
        discovery_entry(
            "tnv3Tool",
            tool_auto("TAU_TNV3", &tnv3_bundle, &tnv3_name),
            tnv3_extra,
            &[],
        ),
        discovery_entry(
            "tacTool",
            tool_auto("TAU_TAC", &tac_bundle, &format!("../tac_ref/{tac_name}")),
            tac_extra,
            &[],
        ),
    ]
}
#[derive(Serialize, Debug, PartialEq, Eq)]
struct AudioTrack {
    /// Zero-based position among the audio streams (what `audioTrack` selects).
    index: u32,
    codec: String,
    channels: u32,
    language: String,
    title: String,
}

/// Parses `ffprobe -select_streams a -of json` output into the audio track list.
fn parse_audio_tracks(json: &[u8]) -> Result<Vec<AudioTrack>, String> {
    let root: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "FFprobe returned unreadable stream data.".to_owned())?;
    let streams = root
        .get("streams")
        .and_then(|v| v.as_array())
        .ok_or("FFprobe returned no stream list.")?;
    let text = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str()).unwrap_or("").to_owned();
    Ok(streams
        .iter()
        .filter(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("audio"))
        .enumerate()
        .map(|(i, s)| AudioTrack {
            index: i as u32,
            codec: text(s.get("codec_name")),
            channels: s.get("channels").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            language: text(s.get("tags").and_then(|t| t.get("language"))),
            title: text(s.get("tags").and_then(|t| t.get("title"))),
        })
        .collect())
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct SubtitleTrack {
    /// Zero-based position among the subtitle streams (what `subtitleTrack` selects).
    index: u32,
    codec: String,
    language: String,
    title: String,
    /// Text subtitles can be converted; image subtitles (PGS, VobSub, DVB) cannot.
    text: bool,
}

fn parse_subtitle_tracks(json: &[u8]) -> Result<Vec<SubtitleTrack>, String> {
    let root: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "FFprobe returned unreadable stream data.".to_owned())?;
    let streams = root
        .get("streams")
        .and_then(|v| v.as_array())
        .ok_or("FFprobe returned no stream list.")?;
    let text = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str()).unwrap_or("").to_owned();
    Ok(streams
        .iter()
        .filter(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("subtitle"))
        .enumerate()
        .map(|(i, s)| {
            let codec = text(s.get("codec_name"));
            let image = ["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle", "xsub", "pgssub", "dvdsub"].contains(&codec.as_str());
            SubtitleTrack {
                index: i as u32,
                language: text(s.get("tags").and_then(|t| t.get("language"))),
                title: text(s.get("tags").and_then(|t| t.get("title"))),
                text: !image,
                codec,
            }
        })
        .collect())
}

#[tauri::command]
async fn list_subtitle_tracks(
    app: tauri::AppHandle,
    input: String,
    dependencies: Dependencies,
) -> Result<Vec<SubtitleTrack>, String> {
    let ffmpeg =
        selected_file(&dependencies.ffmpeg, "FFmpeg")?.unwrap_or_else(|| ffmpeg_path(&app));
    let ffprobe = selected_file(&dependencies.ffprobe, "FFprobe")?
        .unwrap_or_else(|| ffmpeg.with_file_name(executable_name("ffprobe")));
    tauri::async_runtime::spawn_blocking(move || {
        let out = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "s", "-show_entries",
                "stream=index,codec_type,codec_name:stream_tags=language,title", "-of", "json", "-i"])
            .arg(&input)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("Could not start FFprobe: {e}"))?;
        if !out.status.success() {
            return Err("FFprobe could not read the subtitle tracks of this file.".to_owned());
        }
        parse_subtitle_tracks(&out.stdout)
    })
    .await
    .map_err(|e| format!("Track listing stopped: {e}"))?
}

#[tauri::command]
async fn list_audio_tracks(
    app: tauri::AppHandle,
    input: String,
    dependencies: Dependencies,
) -> Result<Vec<AudioTrack>, String> {
    let ffmpeg =
        selected_file(&dependencies.ffmpeg, "FFmpeg")?.unwrap_or_else(|| ffmpeg_path(&app));
    let ffprobe = selected_file(&dependencies.ffprobe, "FFprobe")?
        .unwrap_or_else(|| ffmpeg.with_file_name(executable_name("ffprobe")));
    tauri::async_runtime::spawn_blocking(move || {
        let out = Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "a", "-show_entries",
                "stream=index,codec_type,codec_name,channels:stream_tags=language,title", "-of", "json", "-i"])
            .arg(&input)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("Could not start FFprobe: {e}"))?;
        if !out.status.success() {
            return Err("FFprobe could not read the audio tracks of this file.".to_owned());
        }
        parse_audio_tracks(&out.stdout)
    })
    .await
    .map_err(|e| format!("Track listing stopped: {e}"))?
}

#[tauri::command]
fn experimental_rust_encoder_available() -> bool {
    cfg!(feature = "experimental-rust-encoder")
}
#[tauri::command]
async fn check_source(
    app: tauri::AppHandle,
    input: String,
    audio_track: Option<u32>,
    dependencies: Dependencies,
    state: tauri::State<'_, SourceCheckState>,
) -> Result<SourceCheckResult, String> {
    let ffmpeg =
        selected_file(&dependencies.ffmpeg, "FFmpeg")?.unwrap_or_else(|| ffmpeg_path(&app));
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut current = state
            .0
            .lock()
            .map_err(|_| "Source check state unavailable.")?;
        if current.is_some() {
            return Err("A source check is already running.".into());
        }
        *current = Some(cancel.clone());
    }
    let worker_cancel = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        check_source_impl(&ffmpeg, Path::new(&input), audio_track.unwrap_or(0), &worker_cancel)
    })
    .await;
    if let Ok(mut current) = state.0.lock() {
        if current
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &cancel))
        {
            *current = None;
        }
    }
    result.map_err(|e| format!("Source check stopped unexpectedly: {e}"))?
}

#[tauri::command]
fn stop_source_check(state: tauri::State<'_, SourceCheckState>) -> Result<(), String> {
    let current = state
        .0
        .lock()
        .map_err(|_| "Source check state unavailable.")?;
    if let Some(cancel) = current.as_ref() {
        cancel.store(true, Ordering::Release);
        Ok(())
    } else {
        Err("The source check is still starting or has already finished.".into())
    }
}
fn request_source_decision(
    app: &tauri::AppHandle,
    job_id: &str,
    details: &str,
    control: &JobControl,
) -> Result<(), String> {
    let state = app.state::<SourceDecisionState>();
    let (sender, receiver) = mpsc::sync_channel(1);
    {
        let mut pending = state
            .0
            .lock()
            .map_err(|_| "Source decision state unavailable.")?;
        if pending.is_some() {
            return Err("A source decision is already pending.".into());
        }
        *pending = Some(PendingSourceDecision {
            job_id: job_id.to_owned(),
            reply: sender,
        });
    }
    if app.get_webview_window("main").is_none()
        || app.emit("source-error-decision", SourceErrorDecisionEvent {
            job_id: job_id.to_owned(),
            details: details.to_owned(),
        }).is_err()
    {
        if let Ok(mut pending) = state.0.lock() {
            *pending = None;
        }
        return Err(
            "Could not display the source error; encoding stopped before output publication."
                .into(),
        );
    }
    let result = loop {
        match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(true) => break Ok(()),
            Ok(false) => break Err("Encoding stopped before output files were created.".into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(
                    "Source decision was interrupted; encoding stopped before output publication."
                        .into(),
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) if app.get_webview_window("main").is_none() => {
                break Err(
                    "The encoder window closed; encoding stopped before output publication.".into(),
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) if control.checkpoint().is_err() => {
                let _ = app.emit(
                    "source-error-decision-cancelled",
                    SourceDecisionCancelledEvent { job_id: job_id.to_owned() },
                );
                break Err("The encode stopped while waiting for a source decision.".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        }
    };
    if let Ok(mut pending) = state.0.lock() {
        *pending = None;
    }
    result
}

#[tauri::command]
fn respond_source_error(
    job_id: String,
    proceed: bool,
    state: tauri::State<'_, SourceDecisionState>,
) -> Result<(), String> {
    resolve_source_decision(&state, &job_id, proceed)
}
fn resolve_source_decision(
    state: &SourceDecisionState,
    job_id: &str,
    proceed: bool,
) -> Result<(), String> {
    let job_id = job_id.trim();
    if job_id.is_empty() {
        return Err("A source decision job ID is required.".into());
    }
    let mut pending = state.0.lock().map_err(|_| "Source decision state unavailable.")?;
    let active = pending.as_ref().ok_or("No source decision is pending.")?;
    if active.job_id != job_id {
        return Err("This source decision belongs to a different encode.".into());
    }
    let active = pending.take().ok_or("No source decision is pending.")?;
    active.reply.send(proceed)
        .map_err(|_| "The source decision has already ended.".into())
}
fn cancel_pending_source_decision(state: &SourceDecisionState) -> Result<(), String> {
    if let Some(pending) = state.0.lock()
        .map_err(|_| "Source decision state unavailable.")?.take() {
        let _ = pending.reply.send(false);
    }
    Ok(())
}
#[tauri::command]
fn stop_encode(
    state: tauri::State<'_, ActiveEncode>,
    decisions: tauri::State<'_, SourceDecisionState>,
) -> Result<(), String> {
    let active = state.0.lock().map_err(|_| "Encoder control unavailable.")?;
    active
        .as_ref()
        .ok_or("The encode has not started or has already finished.")?
        .request_stop()?;
    // Resolve an open UI prompt as Stop too. A source-gated native worker must
    // additionally stop its gate and signal its decoder supervisor so a
    // blocked pipe read is released; see native branch notes below.
    cancel_pending_source_decision(&decisions)?;
    Ok(())
}
#[derive(Serialize)]
struct OutputPreview {
    width: u32,
    height: u32,
    minimum_level: u8,
}

fn inspect_output_impl(ffmpeg: &Path, input: &Path, width: u32) -> Result<OutputPreview, String> {
    if !(2..=800).contains(&width) || width % 2 != 0 {
        return Err("Choose an even width from 2 to 800 pixels.".into());
    }
    let filter = format!(
        "scale={width}:720:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24"
    );
    let result = Command::new(ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "yuv4mpegpipe",
            "pipe:1",
        ])
        .output()
        .map_err(|e| format!("Could not inspect video: {e}"))?;
    if !result.status.success() {
        return Err(
            "Could not inspect the source dimensions. Check the source and FFmpeg settings.".into(),
        );
    }
    let end = result
        .stdout
        .iter()
        .position(|&b| b == b'\n')
        .ok_or("FFmpeg returned an invalid video header.")?;
    let header = &result.stdout[..=end];
    let text =
        std::str::from_utf8(header).map_err(|_| "FFmpeg returned an invalid video header.")?;
    let dimension = |key: char| {
        text.split_ascii_whitespace()
            .find_map(|field| field.strip_prefix(key).and_then(|v| v.parse::<u32>().ok()))
    };
    let width = dimension('W').ok_or("Video header has no width.")?;
    let height = dimension('H').ok_or("Video header has no height.")?;
    let minimum_level = (1..=3)
        .find(|&level| {
            tau_video::experimental_c_orchestrator::check_level_fit(header, level).is_ok()
        })
        .ok_or("The output dimensions and frame rate do not fit levels 1–3.")?;
    Ok(OutputPreview {
        width,
        height,
        minimum_level,
    })
}

#[tauri::command]
async fn inspect_output(
    app: tauri::AppHandle,
    input: String,
    width: u32,
    dependencies: Dependencies,
) -> Result<OutputPreview, String> {
    let ffmpeg =
        selected_file(&dependencies.ffmpeg, "FFmpeg")?.unwrap_or_else(|| ffmpeg_path(&app));
    tauri::async_runtime::spawn_blocking(move || {
        inspect_output_impl(&ffmpeg, Path::new(&input), width)
    })
    .await
    .map_err(|e| format!("Could not inspect video: {e}"))?
}
fn workspace_bytes(mib: u32) -> Result<u64, String> {
    if mib == 0 || mib > 16_384 {
        return Err("Workspace limit must be between 1 and 16384 MiB.".into());
    }
    u64::from(mib)
        .checked_mul(1024 * 1024)
        .ok_or_else(|| "Workspace limit is too large.".into())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Parameters are named Tauri command arguments.
async fn encode(
    app: tauri::AppHandle,
    job_id: String,
    input: String,
    folder: String,
    name: String,
    qp: u8,
    workflow: Option<String>,
    mode: Option<String>,
    workspace_mib: Option<u32>,
    continue_source_errors: Option<bool>,
    advanced: AdvancedSettings,
    mut dependencies: Dependencies,
    busy: tauri::State<'_, Mutex<bool>>,
    active: tauri::State<'_, ActiveEncode>,
) -> Result<ResultInfo, String> {
    if job_id.trim().is_empty() {
        return Err("An encode job ID is required.".into());
    }
    let chosen_ffmpeg = selected_file(&dependencies.ffmpeg, "FFmpeg")?;
    let pure_rust = workflow.is_none() && mode.as_deref() == Some("rust");
    if let Some(value) = workflow.as_deref() {
        if value != "python" && value != "native" {
            return Err("Unknown encoder workflow.".into());
        }
    }
    let workflow = workflow.unwrap_or_else(|| "python".into());
    if pure_rust && !cfg!(feature = "experimental-rust-encoder") {
        return Err("The experimental Rust encoder is not enabled in this build.".into());
    }
    let workspace_mib = workspace_mib.unwrap_or(512);
    let workspace_limit_bytes = workspace_bytes(workspace_mib)?;
    dependencies.workspace_limit_bytes = workspace_limit_bytes;
    let chosen_ffprobe = if workflow == "python" && !pure_rust {
        selected_file(&dependencies.ffprobe, "FFprobe")?
    } else {
        None
    };
    let chosen_script = if workflow == "python" && !pure_rust {
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
    let control = Arc::new(JobControl::default());
    {
        let mut job = match active.0.lock() {
            Ok(job) => job,
            Err(_) => {
                if let Ok(mut running) = busy.lock() {
                    *running = false;
                }
                return Err("Encoder control unavailable.".into());
            }
        };
        *job = Some(control.clone());
    }

    let ffmpeg = chosen_ffmpeg.unwrap_or_else(|| ffmpeg_path(&app));
    let ffprobe = chosen_ffprobe.unwrap_or_else(|| {
        ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        })
    });
    let script = if workflow == "python" && !pure_rust {
        Some(chosen_script.unwrap_or_else(|| codec_script(&app)))
    } else {
        None
    };
    let native_event_loop_done = Arc::new(AtomicBool::new(false));
    if workflow == "native" {
        // Tools not chosen in Settings: the environment (TAU_TVC_ENC, TAU_TNV3, TAU_TAC), then the bundled copies,
        // then the repository's own build in a development checkout.
        for (field, env, name, dir) in [
            (&mut dependencies.tvc_encoder, "TAU_TVC_ENC", "tvc_enc", "tvc_ref"),
            (&mut dependencies.tnv3_tool, "TAU_TNV3", "tnv3", "tnv3_ref"),
            (&mut dependencies.tac_tool, "TAU_TAC", "tac", "tac_ref"),
        ] {
            if field.trim().is_empty() {
                let exe = executable_name(name);
                if let Some(found) = locate_tool(&app, env, &format!("codec-tools/{dir}/{exe}"), &format!("../../tools/{dir}/{exe}")) {
                    *field = found.display().to_string();
                }
            }
        }
    }
    let native_event_dispatcher = if workflow == "native" {
        let (events, pending_events) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(
            continue_source_errors.unwrap_or(false),
            events,
        ));
        control.install_source_gate(gate.clone())?;
        let decision_app = app.clone();
        let decision_job_id = job_id.clone();
        let decision_control = control.clone();
        let event_loop_done = native_event_loop_done.clone();
        Some(std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                loop {
                    match pending_events.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(details) => {
                            // Pipe reads can split even the first diagnostic line.
                            // Let the independent drainer collect a short excerpt
                            // before presenting the first warning.
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            if decision_control.checkpoint().is_err() {
                                break;
                            }
                            let details = gate.diagnostics()
                                .ok()
                                .map(|(text, _)| text)
                                .filter(|text| !text.is_empty())
                                .unwrap_or(details);
                            let response = request_source_decision(
                                &decision_app,
                                &decision_job_id,
                                &details,
                                &decision_control,
                            );
                            if gate.decide(response.is_ok()).is_err() || response.is_err() {
                                let _ = decision_control.request_stop();
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout)
                            if event_loop_done.load(Ordering::Acquire) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    }
                }
            }));
            if outcome.is_err() {
                let _ = decision_app.emit(
                    "source-error-decision-cancelled",
                    SourceDecisionCancelledEvent { job_id: decision_job_id.clone() },
                );
                let _ = gate.stop();
                let _ = decision_control.request_stop();
            }
        }))
    } else {
        None
    };
    let dock_app = app.clone();
    let has_subtitles = advanced.subtitle_track.is_some();
    let dock = std::sync::Arc::new(dock_icon::DockIcon::start(&app, has_subtitles));
    let progress_dock = dock.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let reporter = app.clone();
        let decision_app = app.clone();
        let decision_job_id = job_id.clone();
        let callback_control = control.clone();
        let live_native = control.is_live_native();
        let mut accepted = continue_source_errors.unwrap_or(false) || live_native;
        let encoded = encode_pair_impl(
            &ffmpeg,
            Path::new(&input),
            Path::new(&folder),
            &name,
            qp,
            if workflow == "python" && !pure_rust {
                script.as_deref().map(|s| (s, ffprobe.as_path()))
            } else {
                None
            },
            pure_rust,
            advanced,
            &dependencies,
            &control,
            move |details| {
                if live_native {
                    // Native diagnostics were gated at the point they arrived.
                    // This callback remains the legacy post-stage policy for
                    // Python and does not issue a duplicate native prompt.
                    return Ok(());
                }
                if !accepted {
                    request_source_decision(&decision_app, &decision_job_id, details, &callback_control)?;
                    accepted = true;
                }
                Ok(())
            },
            move |event| {
                if cfg!(target_os = "macos") {
                    progress_dock.set(icon_phase(&event));
                } else {
                    show_system_progress(&reporter, Some(&event));
                }
                let _ = reporter.emit("encode-progress", event);
            },
        );
        if encoded.is_err() {
            let _ = control.request_stop();
        }
        encoded
    })
    .await;
    if cfg!(target_os = "macos") {
        dock.finish(matches!(&result, Ok(Ok(_))));
    } else {
        show_system_progress(&dock_app, None);
    }
    native_event_loop_done.store(true, Ordering::Release);
    let dispatcher_panicked = native_event_dispatcher
        .is_some_and(|dispatcher| dispatcher.join().is_err());
    if let Ok(mut job) = active.0.lock() {
        *job = None;
    }
    *busy.lock().map_err(|_| "Encoder state unavailable.")? = false;
    if dispatcher_panicked {
        return Err("Native source decision dispatcher panicked.".into());
    }
    result.map_err(|e| format!("Encoder task stopped: {e}"))?
}
/// Finds a bundled or development tool for decoding embedded audio.
fn locate_tool(app: &tauri::AppHandle, env: &str, bundled: &str, repo: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join(bundled);
        if p.is_file() {
            return Some(p);
        }
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(repo);
    p.is_file().then_some(p)
}

/// Decodes the TAC audio of `path` to a temporary WAV the page can play. Slow for long films.
fn decode_embedded_audio(app: &tauri::AppHandle, path: &Path) -> Result<PathBuf, String> {
    let tnv3 = locate_tool(app, "TAU_TNV3", &format!("codec-tools/tnv3_ref/{}", executable_name("tnv3")),
        &format!("../../tools/tnv3_ref/{}", executable_name("tnv3")))
        .ok_or("The TNV3 tool is needed to play embedded audio; set TAU_TNV3 or bundle it with the app.")?;
    let tac = locate_tool(app, "TAU_TAC", &format!("codec-tools/tac_ref/{}", executable_name("tac")),
        &format!("../../tools/tac_ref/{}", executable_name("tac")))
        .ok_or("The TAC decoder is needed to play embedded audio; set TAU_TAC or bundle it with the app.")?;
    let dir = std::env::temp_dir().join("tau-encoder-player");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare audio cache: {e}"))?;
    let out = dir.join(format!("audio-{}.wav", std::process::id()));
    let _ = std::fs::remove_file(&out);
    tau_video::c_audio::decode_audio(&tau_video::c_audio::Tools { tnv3, tac }, path, &out)
        .map_err(|e| format!("Could not decode the embedded audio: {e}"))?;
    Ok(out)
}

#[tauri::command]
async fn player_open(
    app: tauri::AppHandle,
    path: String,
    state: tauri::State<'_, Mutex<Option<player::Player>>>,
) -> Result<player::PlayerInfo, String> {
    let worker_app = app.clone();
    let (player, mut info) = tauri::async_runtime::spawn_blocking(move || {
        let (player, mut info) = player::Player::open(Path::new(&path))?;
        if info.audio_source == Some("embedded-tac") {
            let wav = decode_embedded_audio(&worker_app, Path::new(&path))?;
            info.audio = Some(wav.to_string_lossy().into_owned());
        }
        Ok::<_, String>((player, info))
    })
    .await
    .map_err(|e| format!("Player stopped: {e}"))??;
    if let Some(ref audio) = info.audio {
        app.asset_protocol_scope()
            .allow_file(audio)
            .map_err(|e| e.to_string())?;
    }
    let _ = &mut info;
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
/// Subtitle overlay for the preview. Reply: `u32 key`; when the key differs from `known_key` also
/// `u16 x, y, w, h` (grid box) and `w * h * 4` bytes of straight-alpha RGBA (empty box: nothing drawn).
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Named Tauri command arguments.
fn player_subtitle(
    track: u32,
    variant: u32,
    position: i64,
    subtitles_on: bool,
    hide_sdh: bool,
    known_key: u32,
    state: tauri::State<'_, Mutex<Option<player::Player>>>,
) -> Result<tauri::ipc::Response, String> {
    let locked = state.lock().map_err(|_| "Player state unavailable.")?;
    let player = locked.as_ref().ok_or("Open a video first.")?;
    let o = player.subtitle_overlay(track as usize, variant as usize, position, subtitles_on, hide_sdh, known_key)?;
    let mut out = o.key.to_le_bytes().to_vec();
    if o.key != known_key {
        for v in [o.x, o.y, o.w, o.h] {
            out.extend(v.to_le_bytes());
        }
        out.extend(o.rgba);
    }
    Ok(tauri::ipc::Response::new(out))
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
        .manage(SourceCheckState::default())
        .manage(SourceDecisionState::default())
        .manage(ActiveEncode::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            experimental_rust_encoder_available,
            list_audio_tracks,
            list_subtitle_tracks,
            check_source,
            stop_source_check,
            respond_source_error,
            stop_encode,
            inspect_output,
            discover_tools,
            encode,
            player_open,
            player_next,
            player_subtitle,
            player_close
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Tau Encoder");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_mp3() -> AdvancedSettings {
        AdvancedSettings { audio: AudioFormat::Mp3, ..Default::default() }
    }

    #[test]
    fn stop_signals_source_even_if_gate_state_is_poisoned() {
        let control = Arc::new(JobControl::default());
        let broken = control.clone();
        let _ = std::thread::spawn(move || {
            let _locked = broken.source_gate.lock().unwrap();
            panic!("poison gate state");
        }).join();
        let (send, receive) = mpsc::channel();
        control.install_source_stop(send).unwrap();
        assert!(control.request_stop().is_err());
        assert!(receive.recv_timeout(std::time::Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn source_decision_rejects_stale_job_id_and_keeps_pending_reply() {
        let (reply, receiver) = mpsc::sync_channel(1);
        let state = SourceDecisionState(Mutex::new(Some(PendingSourceDecision {
            job_id: "current-job".into(),
            reply,
        })));
        assert!(resolve_source_decision(&state, "old-job", true).is_err());
        assert!(receiver.try_recv().is_err());
        resolve_source_decision(&state, "current-job", true).unwrap();
        assert!(receiver.recv().unwrap());
    }

    #[test]
    fn stop_encode_resolves_waiting_prompt_as_decline() {
        let (reply, receiver) = mpsc::sync_channel(1);
        let state = SourceDecisionState(Mutex::new(Some(PendingSourceDecision {
            job_id: "active-job".into(),
            reply,
        })));
        cancel_pending_source_decision(&state).unwrap();
        assert!(!receiver.recv().unwrap());
        assert!(state.0.lock().unwrap().is_none());
    }

    #[test]
    fn source_event_uses_frontend_job_id_wire_shape() {
        let value = serde_json::to_value(SourceErrorDecisionEvent {
            job_id: "job-123".into(),
            details: "decoder warning".into(),
        }).unwrap();
        assert_eq!(value["job_id"], "job-123");
        assert_eq!(value["details"], "decoder warning");
        assert!(value.get("jobId").is_none());
        let cancelled = serde_json::to_value(SourceDecisionCancelledEvent {
            job_id: "job-123".into(),
        }).unwrap();
        assert_eq!(cancelled["job_id"], "job-123");
    }

    #[test]
    fn native_source_decline_fails_gate_and_retains_error_excerpt() {
        let (events, pending) = mpsc::channel();
        let gate = SourceGate::new(false, events);
        gate.report(b"[matroska] invalid as first byte").unwrap();
        assert!(pending.recv().unwrap().contains("invalid as first byte"));
        gate.decide(false).unwrap();
        assert!(gate.checkpoint().is_err());
        let (details, _) = gate.diagnostics().unwrap();
        assert!(details.contains("invalid as first byte"));
    }

    #[cfg(unix)]
    #[test]
    fn native_app_gate_stop_publishes_nothing_and_continue_carries_across_video_audio() {
        use std::{io::Write, os::unix::fs::PermissionsExt};
        let (Some(real_ffmpeg), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };

        let (source, file) = temporary_file("mp4").unwrap();
        drop(file);
        assert!(Command::new(&real_ffmpeg)
            .args([
                "-y", "-v", "error", "-f", "lavfi", "-i",
                "testsrc2=size=64x48:rate=24:duration=0.5", "-f", "lavfi", "-i",
                "sine=frequency=440:sample_rate=48000:duration=0.5", "-c:v", "mpeg4",
                "-q:v", "4", "-c:a", "aac",
            ])
            .arg(&source.0)
            .status()
            .unwrap()
            .success());

        let (wrapper, mut script_file) = temporary_file("sh").unwrap();
        let escaped_ffmpeg = PathBuf::from(&real_ffmpeg)
            .display()
            .to_string()
            .replace('\'', "'\\''");
        writeln!(
            script_file,
            "#!/bin/sh\nprintf '[matroska,webm @ 0] controlled source warning\\n' >&2\nexec '{escaped_ffmpeg}' \"$@\""
        )
        .unwrap();
        drop(script_file);
        std::fs::set_permissions(&wrapper.0, std::fs::Permissions::from_mode(0o700)).unwrap();

        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(&tvc).display().to_string(),
            tnv3_tool: PathBuf::from(&tnv3).display().to_string(),
            workspace_limit_bytes: 512 * 1024 * 1024,
            ..Default::default()
        };
        let run_with_decision = |name: &str, proceed: bool| {
            let (events, pending) = mpsc::channel();
            let gate = Arc::new(SourceGate::new(false, events));
            let control = Arc::new(JobControl::default());
            control.install_source_gate(gate.clone()).unwrap();
            let dispatch_gate = gate.clone();
            let dispatch_control = control.clone();
            let dispatcher = std::thread::spawn(move || {
                let details = pending
                    .recv_timeout(std::time::Duration::from_secs(30))
                    .expect("controlled FFmpeg warning should request a decision");
                dispatch_gate.decide(proceed).unwrap();
                if !proceed {
                    dispatch_control.request_stop().unwrap();
                }
                details
            });
            let (reserve, file) = temporary_file("tnv3").unwrap();
            drop(file);
            std::fs::remove_file(&reserve.0).unwrap();
            let name = format!("{name}_{}", reserve.0.file_stem().unwrap().to_string_lossy());
            let folder = reserve.0.parent().unwrap().to_path_buf();
            let result = encode_pair_impl(
                &wrapper.0,
                &source.0,
                &folder,
                &name,
                30,
                None,
                false,
                legacy_mp3(),
                &dependencies,
                &control,
                |_| Ok(()),
                |_| {},
            );
            let details = dispatcher.join().unwrap();
            (result, folder.join(format!("{name}.tnv3")), folder.join(format!("{name}.mp3")), details)
        };

        let (declined, declined_video, declined_audio, decline_details) =
            run_with_decision("declined", false);
        assert!(decline_details.contains("controlled source warning"));
        assert!(matches!(declined, Err(message) if message.contains("stopped")));
        assert!(!declined_video.exists(), "Stop must not publish TNV3");
        assert!(!declined_audio.exists(), "Stop must not publish MP3");

        let (continued, video_path, audio_path, continue_details) =
            run_with_decision("continued", true);
        let continued = continued.expect("Continue should finish the native pair");
        assert!(continue_details.contains("controlled source warning"));
        assert!(continued.warnings.iter().any(|warning| warning.contains("Source decoding")));
        assert!(continued.warnings.iter().any(|warning| warning.contains("Audio decoding")));
        assert!(video_path.is_file() && audio_path.is_file());
        assert!(tau_video::Tnv3Reader::open(File::open(&video_path).unwrap())
            .unwrap()
            .info()
            .frame_count > 0);
        assert!(std::fs::metadata(&audio_path).unwrap().len() > 1_000);
        let _video = Temporary(video_path);
        let _audio = Temporary(audio_path);
    }

    #[cfg(unix)]
    #[test]
    fn native_recovery_source_first_warning_decline_stops_without_pair_publication() {
        let (Some(source), Some(real_ffmpeg), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_RECOVERY_SOURCE"),
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let source = PathBuf::from(source);
        if !source.is_file() {
            return;
        }
        let expected_frames = estimated_frames(Path::new(&real_ffmpeg), &source);

        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(&tvc).display().to_string(),
            tnv3_tool: PathBuf::from(&tnv3).display().to_string(),
            workspace_limit_bytes: 512 * 1024 * 1024,
            ..Default::default()
        };
        let (events, pending) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(false, events));
        let control = Arc::new(JobControl::default());
        control.install_source_gate(gate.clone()).unwrap();
        let dispatch_gate = gate.clone();
        let dispatch_control = control.clone();
        let dispatcher = std::thread::spawn(move || {
            let details = pending
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("native recovery decode should report the source warning");
            dispatch_gate.decide(false).unwrap();
            dispatch_control.request_stop().unwrap();
            details
        });

        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_string_lossy().into_owned();
        let folder = reserve.0.parent().unwrap().to_path_buf();
        let mut max_frames = 0u32;
        let result = encode_pair_impl(
            Path::new(&real_ffmpeg),
            &source,
            &folder,
            &name,
            30,
            None,
            false,
            legacy_mp3(),
            &dependencies,
            &control,
            |_| Ok(()),
            |event| max_frames = max_frames.max(event.frames),
        );
        let details = dispatcher.join().unwrap();
        assert!(!details.trim().is_empty(), "source warning should not be empty");
        assert!(matches!(result, Err(message) if message.contains("stopped")));
        assert!(!reserve.0.exists(), "Stop must not publish TNV3");
        assert!(!reserve.0.with_extension("mp3").exists(), "Stop must not publish MP3");
        if let Some(expected_frames) = expected_frames {
            assert!(max_frames < expected_frames, "declining the first event should stop before the source is fully encoded (observed {max_frames} of about {expected_frames} frames)");
        }
    }
    #[test]
    fn stop_and_publication_are_mutually_exclusive() {
        let stopped = JobControl::default();
        stopped.request_stop().unwrap();
        assert!(stopped.checkpoint().is_err());
        assert!(stopped.begin_publish().is_err());
        let publishing = JobControl::default();
        publishing.begin_publish().unwrap();
        assert!(publishing.request_stop().is_err());
        for _ in 0..100 {
            let control = Arc::new(JobControl::default());
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let worker_control = control.clone();
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                worker_control.request_stop()
            });
            barrier.wait();
            let publish = control.begin_publish();
            let stop = worker.join().unwrap();
            assert!(
                !(stop.is_ok() && publish.is_ok()),
                "stop and publication must not both be accepted"
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn python_phase_warning_can_stop_before_output_publication() {
        use std::{io::Write, os::unix::fs::PermissionsExt};
        let (Some(ffmpeg), Some(ffprobe), Some(script), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_FFPROBE"),
            std::env::var_os("TAU_TEST_C_SCRIPT"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let (source, file) = temporary_file("mp4").unwrap();
        drop(file);
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=64x48:rate=24:duration=0.25",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=0.25",
                    "-c:v",
                    "mpeg4",
                    "-c:a",
                    "aac"
                ])
                .arg(&source.0)
                .status()
                .unwrap()
                .success()
        );
        let (wrapper, mut file) = temporary_file("sh").unwrap();
        let escaped = PathBuf::from(&ffmpeg)
            .display()
            .to_string()
            .replace('\'', "'\\''");
        write!(file, "#!/bin/sh\nprintf '[matroska,webm @ 0] Duplicate element\\n' >&2\nexec '{escaped}' \"$@\"\n").unwrap();
        drop(file);
        std::fs::set_permissions(&wrapper.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        let deps = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            ..Default::default()
        };
        // Rust+C uses the live gate and is covered by
        // native_app_gate_stop_publishes_nothing_and_continue_carries_across_video_audio.
        // Keep this test focused on the legacy Python callback behavior.
        for python in [true] {
            let (reserve, file) = temporary_file("tnv3").unwrap();
            drop(file);
            std::fs::remove_file(&reserve.0).unwrap();
            let name = reserve.0.file_stem().unwrap().to_str().unwrap();
            let tools = if python {
                Some((Path::new(&script), Path::new(&ffprobe)))
            } else {
                None
            };
            let mut seen = false;
            let result = encode_pair_impl(
                &wrapper.0,
                &source.0,
                reserve.0.parent().unwrap(),
                name,
                30,
                tools,
                false,
                legacy_mp3(),
                &deps,
                &JobControl::default(),
                |details| {
                    seen = details.contains("Duplicate element");
                    Err("Encoding stopped before output files were created.".into())
                },
                |_| {},
            );
            assert!(seen);
            assert!(result.is_err());
            assert!(!reserve.0.exists());
            assert!(!reserve.0.with_extension("mp3").exists());
            let control = JobControl::default();
            let result = encode_pair_impl(
                &wrapper.0,
                &source.0,
                reserve.0.parent().unwrap(),
                name,
                30,
                tools,
                false,
                legacy_mp3(),
                &deps,
                &control,
                |_| panic!("a requested stop should take precedence over a warning prompt"),
                |progress| {
                    if progress.frames > 0
                        && (progress.phase == "video" || progress.phase == "c-video")
                    {
                        control.request_stop().unwrap();
                    }
                },
            );
            assert!(matches!(result, Err(message) if message.starts_with("Encoding stopped")));
            assert!(!reserve.0.exists());
            assert!(!reserve.0.with_extension("mp3").exists());
            let mut confirmations = 0;
            let result = encode_pair_impl(
                &wrapper.0,
                &source.0,
                reserve.0.parent().unwrap(),
                name,
                30,
                tools,
                false,
                legacy_mp3(),
                &deps,
                &JobControl::default(),
                |_| {
                    confirmations += 1;
                    Ok(())
                },
                |_| {},
            )
            .unwrap();
            assert!(confirmations >= 2);
            let video = Temporary(PathBuf::from(result.video));
            let audio = Temporary(PathBuf::from(result.audio));
            assert!(video.0.is_file() && audio.0.is_file());
        }
    }
    #[test]
    fn source_check_reports_configured_corrupt_source() {
        let (Some(ffmpeg), Some(source)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_RECOVERY_SOURCE"),
        ) else {
            return;
        };
        let result = check_source_impl(
            Path::new(&ffmpeg),
            Path::new(&source),
            0,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.status, "warning");
        assert!(!result.details.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn source_check_handles_clean_warning_failure_and_stop() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        fn stub(script: &str) -> Temporary {
            let (file, mut out) = temporary_file("sh").unwrap();
            write!(out, "#!/bin/sh\n{script}\n").unwrap();
            drop(out);
            std::fs::set_permissions(&file.0, std::fs::Permissions::from_mode(0o700)).unwrap();
            file
        }
        let clean = stub("exit 0");
        assert_eq!(
            check_source_impl(&clean.0, Path::new("unused"), 0, &AtomicBool::new(false))
                .unwrap()
                .status,
            "clean"
        );
        let warning =
            stub("printf '[matroska,webm @ 0] Duplicate element\\n' >&2; exec /bin/sleep 20");
        let result =
            check_source_impl(&warning.0, Path::new("unused"), 0, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.status, "warning");
        assert!(result.details.contains("Duplicate element"));
        let failed = stub("printf 'fatal input error\\n' >&2; exit 2");
        assert!(
            check_source_impl(&failed.0, Path::new("unused"), 0, &AtomicBool::new(false)).is_err()
        );
        let slow = stub("exec /bin/sleep 20");
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            signal.store(true, Ordering::Release);
        });
        let result = check_source_impl(&slow.0, Path::new("unused"), 0, &cancel).unwrap();
        stopper.join().unwrap();
        assert_eq!(result.status, "stopped");
        let closed = stub("exec 2>&-; exec /bin/sleep 20");
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            signal.store(true, Ordering::Release);
        });
        let result = check_source_impl(&closed.0, Path::new("unused"), 0, &cancel).unwrap();
        stopper.join().unwrap();
        assert_eq!(result.status, "stopped");
    }
    #[test]
    fn portrait_preview_preserves_aspect_and_height_limit() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else {
            return;
        };
        let (source, file) = temporary_file("mp4").unwrap();
        drop(file);
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x600:rate=24",
                    "-frames:v",
                    "1",
                    "-c:v",
                    "mpeg4"
                ])
                .arg(&source.0)
                .status()
                .unwrap()
                .success()
        );
        let preview = inspect_output_impl(Path::new(&ffmpeg), &source.0, 240).unwrap();
        assert_eq!(
            (preview.width, preview.height, preview.minimum_level),
            (192, 720, 3)
        );
    }
    #[test]
    fn c_workflows_honor_resolution_and_levels() {
        let (Some(ffmpeg), Some(ffprobe), Some(script), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_FFPROBE"),
            std::env::var_os("TAU_TEST_C_SCRIPT"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let (source_guard, file) = temporary_file("mp4").unwrap();
        drop(file);
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x240:rate=24:duration=0.25",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=0.25",
                    "-c:v",
                    "mpeg4",
                    "-c:a",
                    "aac"
                ])
                .arg(&source_guard.0)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            check_source_impl(Path::new(&ffmpeg), &source_guard.0, 0, &AtomicBool::new(false))
                .unwrap()
                .status,
            "clean"
        );
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            ..Default::default()
        };
        for backend in [false, true] {
            for (width, expected_height, level) in [(240, 180, 1), (360, 270, 2)] {
                let preview =
                    inspect_output_impl(Path::new(&ffmpeg), &source_guard.0, width).unwrap();
                assert_eq!(
                    (preview.width, preview.height, preview.minimum_level),
                    (width, expected_height, level)
                );
                let (reserve, file) = temporary_file("tnv3").unwrap();
                drop(file);
                std::fs::remove_file(&reserve.0).unwrap();
                let settings = AdvancedSettings {
                    audio: AudioFormat::Mp3,
                    width: Some(width),
                    height: Some(720),
                    ..Default::default()
                };
                let tools = if backend {
                    Some((Path::new(&script), Path::new(&ffprobe)))
                } else {
                    None
                };
                let result = encode_pair(
                    Path::new(&ffmpeg),
                    &source_guard.0,
                    reserve.0.parent().unwrap(),
                    reserve.0.file_stem().unwrap().to_str().unwrap(),
                    30,
                    tools,
                    settings,
                    &dependencies,
                    |_| {},
                )
                .unwrap();
                let video = Temporary(PathBuf::from(result.video));
                let _audio = Temporary(PathBuf::from(result.audio));
                let info = tau_video::Tnv3Reader::open(File::open(&video.0).unwrap())
                    .unwrap()
                    .info()
                    .clone();
                assert_eq!(
                    (u32::from(info.width), u32::from(info.height), info.level),
                    (preview.width, preview.height, preview.minimum_level)
                );
            }
            for (width, height, level) in [(400, 240, 1), (800, 464, 2), (800, 720, 3)] {
                let (reserve, file) = temporary_file("tnv3").unwrap();
                drop(file);
                std::fs::remove_file(&reserve.0).unwrap();
                let name = reserve.0.file_stem().unwrap().to_str().unwrap();
                let settings = AdvancedSettings {
                    audio: AudioFormat::Mp3,
                    width: Some(width),
                    height: Some(height),
                    level: Some(level),
                    ..Default::default()
                };
                let tools = if backend {
                    Some((Path::new(&script), Path::new(&ffprobe)))
                } else {
                    None
                };
                let result = encode_pair(
                    Path::new(&ffmpeg),
                    &source_guard.0,
                    reserve.0.parent().unwrap(),
                    name,
                    30,
                    tools,
                    settings,
                    &dependencies,
                    |_| {},
                )
                .unwrap();
                assert_eq!(result.level, level);
                let video = Temporary(PathBuf::from(result.video));
                let _audio = Temporary(PathBuf::from(result.audio));
                let info = tau_video::Tnv3Reader::open(File::open(&video.0).unwrap())
                    .unwrap()
                    .info()
                    .clone();
                assert_eq!(info.level, level);
                assert!(u32::from(info.width) <= width && u32::from(info.height) <= height);
                assert!(info.width > 0 && info.height > 0);
            }
            let (reserve, file) = temporary_file("tnv3").unwrap();
            drop(file);
            std::fs::remove_file(&reserve.0).unwrap();
            let name = reserve.0.file_stem().unwrap().to_str().unwrap();
            let tools = if backend {
                Some((Path::new(&script), Path::new(&ffprobe)))
            } else {
                None
            };
            let settings = AdvancedSettings {
                audio: AudioFormat::Mp3,
                width: Some(800),
                height: Some(720),
                level: Some(1),
                ..Default::default()
            };
            assert!(
                encode_pair(
                    Path::new(&ffmpeg),
                    &source_guard.0,
                    reserve.0.parent().unwrap(),
                    name,
                    30,
                    tools,
                    settings,
                    &dependencies,
                    |_| {}
                )
                .is_err()
            );
        }
    }
    #[test]
    fn tool_discovery_lists_existing_paths_once_and_ignores_missing_files() {
        let (first, file) = temporary_file("tool").unwrap();
        drop(file);
        let (second, file) = temporary_file("tool").unwrap();
        drop(file);
        let missing = first.0.with_extension("missing");
        let result = discovery_entry(
            "example",
            Some(first.0.clone()),
            vec![first.0.clone(), second.0.clone(), missing],
            &[],
        );
        assert_eq!(result.automatic, first.0.to_string_lossy());
        assert_eq!(result.candidates.len(), 2);
        assert!(
            result
                .candidates
                .contains(&second.0.to_string_lossy().to_string())
        );
        assert!(
            discovery_entry("example", None, vec![], &[])
                .automatic
                .is_empty()
        );
    }
    fn test_output_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tau-output-preflight-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn preflight_rejects_nonexistent_folder_without_creating_residue() {
        let root = test_output_dir("invalid");
        let missing = root.join("missing");
        let result = preflight_output_directory(
            &missing,
            &missing.join("movie.tnv3"),
            &missing.join("movie.mp3"),
        );
        assert!(matches!(result, Err(error) if error.contains("existing output folder")));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn encode_rejects_invalid_folder_before_starting_probe() {
        let root = test_output_dir("early");
        let missing = root.join("missing");
        let result = encode_pair(
            Path::new("/missing/ffmpeg"),
            Path::new("/missing/input.mp4"),
            &missing,
            "movie",
            30,
            None,
            legacy_mp3(),
            &Dependencies::default(),
            |_| {},
        );
        assert!(matches!(result, Err(error) if error.contains("existing output folder")));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn preflight_rejects_existing_output_and_preserves_it() {
        let folder = test_output_dir("occupied");
        let video = folder.join("movie.tnv3");
        let audio = folder.join("movie.mp3");
        std::fs::write(&video, b"keep video bytes").unwrap();
        std::fs::write(&audio, b"keep audio bytes").unwrap();
        let result = preflight_output_directory(&folder, &video, &audio);
        assert!(matches!(result, Err(error) if error.contains("already exists")));
        assert_eq!(std::fs::read(&video).unwrap(), b"keep video bytes");
        assert_eq!(std::fs::read(&audio).unwrap(), b"keep audio bytes");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn blocked_existing_output_returns_before_ffmpeg_or_progress() {
        use std::cell::Cell;
        let folder = test_output_dir("early-occupied");
        let video = folder.join("movie.tnv3");
        std::fs::write(&video, b"do not replace").unwrap();
        let progress_seen = Cell::new(false);
        let result = encode_pair(
            Path::new("/missing/ffmpeg"),
            Path::new("/missing/input.mp4"),
            &folder,
            "movie",
            30,
            None,
            legacy_mp3(),
            &Dependencies::default(),
            |_| progress_seen.set(true),
        );
        assert!(matches!(result, Err(error) if error.contains("already exists")));
        assert!(!progress_seen.get());
        assert_eq!(std::fs::read(&video).unwrap(), b"do not replace");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preflight_detects_dangling_symlink_without_changing_it() {
        use std::os::unix::fs::symlink;
        let folder = test_output_dir("dangling");
        let video = folder.join("movie.tnv3");
        let missing_target = folder.join("missing-target");
        symlink(&missing_target, &video).unwrap();
        let result = preflight_output_directory(&folder, &video, &folder.join("movie.mp3"));
        assert!(matches!(result, Err(error) if error.contains("already exists")));
        let metadata = std::fs::symlink_metadata(&video).unwrap();
        assert!(metadata.file_type().is_symlink());
        assert!(!missing_target.exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn preflight_accepts_existing_directory_without_creating_files() {
        let folder = test_output_dir("valid");
        let result = preflight_output_directory(
            &folder,
            &folder.join("movie.tnv3"),
            &folder.join("movie.mp3"),
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        std::fs::remove_dir(folder).unwrap();
    }

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
                legacy_mp3(),
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
            legacy_mp3(),
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
                ("default", legacy_mp3()),
                (
                    "down",
                    AdvancedSettings {
                        audio: AudioFormat::Mp3,
                        round_down: true,
                        ..Default::default()
                    },
                ),
                (
                    "pass",
                    AdvancedSettings {
                        audio: AudioFormat::Mp3,
                        round_down: true,
                        eof_pass: true,
                        ..Default::default()
                    },
                ),
                (
                    "trim",
                    AdvancedSettings {
                        audio: AudioFormat::Mp3,
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
    fn workspace_limit_accepts_maximum_without_narrowing_overflow() {
        assert_eq!(workspace_bytes(512).unwrap(), 512u64 * 1024 * 1024);
        assert_eq!(workspace_bytes(16_384).unwrap(), 16_384u64 * 1024 * 1024);
        assert!(workspace_bytes(0).is_err());
        assert!(workspace_bytes(16_385).is_err());
    }

    #[test]
    fn experimental_pure_rust_availability_matches_build_feature() {
        assert_eq!(
            experimental_rust_encoder_available(),
            cfg!(feature = "experimental-rust-encoder")
        );
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
            tac_tool: String::new(),
            subtitle_font: String::new(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
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
            legacy_mp3(),
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
    /// Guard against silent skipping: most real-media tests below return early without their tools.
    #[test]
    fn reference_tools_are_configured_or_skipping_is_explicit() {
        if std::env::var("TAU_ALLOW_SKIP").is_ok_and(|v| v == "1") {
            eprintln!("TAU_ALLOW_SKIP=1: real-media tests without their tools return early and are NOT checked.");
            return;
        }
        let required = ["TAU_TEST_FFMPEG", "TAU_TEST_FFPROBE", "TAU_TEST_TVC", "TAU_TEST_TNV3", "TAU_TEST_TAC", "TAU_TEST_FONT", "TAU_TEST_C_SCRIPT", "TAU_TEST_PYTHON"];
        let missing: Vec<&str> = required
            .into_iter()
            .filter(|n| std::env::var_os(n).is_none_or(|v| v != "python3" && !Path::new(&v).exists()))
            .collect();
        assert!(missing.is_empty(), "These variables are unset or point nowhere: {missing:?}. Run `make rust-conformance`, or set TAU_ALLOW_SKIP=1 to skip on purpose.");
    }
    #[test]
    fn system_progress_maps_phases_to_percent_or_indeterminate() {
        let p = |phase, frames, total| Progress { phase, frames, total_frames_estimate: total, remaining_seconds: None };
        assert_eq!(system_progress_percent(&p("video", 0, Some(100))), Some(0));
        assert_eq!(system_progress_percent(&p("video", 50, Some(100))), Some(45));
        assert_eq!(system_progress_percent(&p("video", 100, Some(100))), Some(90));
        assert_eq!(system_progress_percent(&p("video", 250, Some(100))), Some(90), "estimate overshoot is clamped");
        assert_eq!(system_progress_percent(&p("c-video", 3, Some(6))), Some(45));
        assert_eq!(system_progress_percent(&p("video", 10, None)), None, "unknown total: indeterminate");
        assert_eq!(system_progress_percent(&p("video", 10, Some(0))), None);
        assert_eq!(system_progress_percent(&p("probing", 0, None)), None);
        assert_eq!(system_progress_percent(&p("audio", 0, None)), Some(92));
        assert_eq!(system_progress_percent(&p("saving", 0, None)), Some(98));
    }
    #[test]
    fn audio_track_list_numbers_only_audio_streams_and_reads_tags() {
        let json = br#"{"streams":[{"index":1,"codec_type":"audio","codec_name":"aac","channels":2,"tags":{"language":"eng"}},{"index":2,"codec_type":"audio","codec_name":"ac3","channels":6,"tags":{"language":"jpn","title":"Commentary"}},{"index":3,"codec_type":"audio","codec_name":"opus"}]}"#;
        let tracks = parse_audio_tracks(json).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!((tracks[1].index, tracks[1].channels, tracks[1].language.as_str(), tracks[1].title.as_str()), (1, 6, "jpn", "Commentary"));
        assert_eq!((tracks[2].channels, tracks[2].language.as_str()), (0, ""));
        assert!(parse_audio_tracks(b"not json").is_err());
        assert!(parse_audio_tracks(b"{}").is_err());
        assert!(parse_audio_tracks(br#"{"streams":[]}"#).unwrap().is_empty());
    }
    #[test]
    fn subtitle_track_list_flags_image_subtitles() {
        let json = br#"{"streams":[{"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}},{"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"jpn","title":"Signs"}},{"codec_type":"subtitle","codec_name":"ass"}]}"#;
        let tracks = parse_subtitle_tracks(json).unwrap();
        assert_eq!(tracks.iter().map(|t| t.text).collect::<Vec<_>>(), [true, false, true]);
        assert_eq!((tracks[1].index, tracks[1].language.as_str(), tracks[1].title.as_str()), (1, "jpn", "Signs"));
        assert!(parse_subtitle_tracks(b"nope").is_err());
    }
    #[test]
    fn real_media_selected_subtitle_track_is_authored_into_the_file() {
        let (Some(ffmpeg), Some(tvc), Some(tnv3), Some(tac), Some(builder)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
            std::env::var_os("TAU_TEST_TAC"),
            std::env::var_os("TAU_TEST_FONT"),
        ) else {
            return;
        };
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(&tnv3).display().to_string(),
            tac_tool: PathBuf::from(tac).display().to_string(),
            subtitle_font: PathBuf::from(builder).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
        };
        let (srt, f) = temporary_file("srt").unwrap();
        drop(f);
        std::fs::write(&srt.0, "1\n00:00:00,200 --> 00:00:01,000\nEnglish\n\n2\n00:00:01,200 --> 00:00:01,900\nHello <i>there</i>\n").unwrap();
        let (src, f) = temporary_file("mkv").unwrap();
        drop(f);
        assert!(Command::new(&ffmpeg).args([
            "-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=2",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-i",
        ]).arg(&srt.0).args(["-map", "0", "-map", "1", "-map", "2", "-c:v", "mpeg4", "-c:a", "aac", "-c:s", "srt"]).arg(&src.0).status().unwrap().success());
        let (reserve, f) = temporary_file("tnv3").unwrap();
        drop(f);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        let settings = AdvancedSettings { subtitle_track: Some(0), ..Default::default() };
        let result = encode_pair(Path::new(&ffmpeg), &src.0, folder, name, 30, None, settings, &dependencies, |_| {}).unwrap();
        let video = Temporary(PathBuf::from(&result.video));
        let info = tau_video::Tnv3Reader::open(File::open(&video.0).unwrap()).unwrap().info().clone();
        assert!(info.flags & 4 != 0 && info.subtitle_length > 0, "subtitle area present");
        let check = Command::new(&tnv3).arg("check").arg(&video.0).output().unwrap();
        assert!(String::from_utf8_lossy(&check.stdout).contains("valid"), "{check:?}");
        // with no font configured the built-in Roboto is used
        let builtin = Dependencies { subtitle_font: String::new(), ..dependencies.clone() };
        let (reserve2, f) = temporary_file("tnv3").unwrap();
        drop(f);
        std::fs::remove_file(&reserve2.0).unwrap();
        let name2 = reserve2.0.file_stem().unwrap().to_str().unwrap();
        let prior = std::env::var_os("TAU_SUBTITLE_FONT");
        // SAFETY: only this test touches the variable.
        unsafe { std::env::remove_var("TAU_SUBTITLE_FONT") };
        let again = encode_pair(Path::new(&ffmpeg), &src.0, folder, name2, 30, None, settings, &builtin, |_| {}).unwrap();
        if let Some(p) = prior {
            unsafe { std::env::set_var("TAU_SUBTITLE_FONT", p) };
        }
        let video2 = Temporary(PathBuf::from(&again.video));
        let info2 = tau_video::Tnv3Reader::open(File::open(&video2.0).unwrap()).unwrap().info().clone();
        assert!(info2.flags & 4 != 0 && info2.subtitle_length > 0, "built-in font authored subtitles");
    }
    #[test]
    fn real_media_selected_audio_track_is_the_one_encoded() {
        let (Some(ffmpeg), Some(tvc), Some(tnv3), Some(tac)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
            std::env::var_os("TAU_TEST_TAC"),
        ) else {
            return;
        };
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(&tnv3).display().to_string(),
            tac_tool: PathBuf::from(&tac).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
        };
        let (src_guard, src_file) = temporary_file("mkv").unwrap();
        drop(src_file);
        assert!(Command::new(&ffmpeg).args([
            "-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=2",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2",
            "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000:duration=2",
            "-map", "0", "-map", "1", "-map", "2", "-c:v", "mpeg4", "-c:a", "aac",
        ]).arg(&src_guard.0).status().unwrap().success());
        let mut found = Vec::new();
        for track in [0u32, 1] {
            let (reserve, f) = temporary_file("tnv3").unwrap();
            drop(f);
            std::fs::remove_file(&reserve.0).unwrap();
            let name = reserve.0.file_stem().unwrap().to_str().unwrap();
            let folder = reserve.0.parent().unwrap();
            let settings = AdvancedSettings { audio_track: Some(track), ..Default::default() };
            let result = encode_pair(Path::new(&ffmpeg), &src_guard.0, folder, name, 30, None, settings, &dependencies, |_| {}).unwrap();
            let video = Temporary(PathBuf::from(&result.video));
            let (tac_out, f) = temporary_file("tac").unwrap();
            drop(f);
            let (wav, f) = temporary_file("wav").unwrap();
            drop(f);
            assert!(Command::new(&tnv3).arg("demux").arg(&video.0).arg("/dev/null").arg(&tac_out.0).status().unwrap().success());
            assert!(Command::new(&tac).arg("dec").arg(&tac_out.0).arg(&wav.0).status().unwrap().success());
            let bytes = std::fs::read(&wav.0).unwrap();
            let samples: Vec<i16> = bytes[44..].chunks_exact(4).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            let crossings = samples.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
            found.push((crossings as f64 / (samples.len() as f64 / 48_000.0)).round() as u32);
        }
        assert!((found[0] as i32 - 440).abs() < 25, "track 0 was {} Hz", found[0]);
        assert!((found[1] as i32 - 1000).abs() < 40, "track 1 was {} Hz", found[1]);
    }
    #[test]
    fn real_media_default_embeds_tac_audio_in_one_file() {
        let (Some(ffmpeg), Some(tvc), Some(tnv3), Some(tac)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
            std::env::var_os("TAU_TEST_TAC"),
        ) else {
            return;
        };
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(&tnv3).display().to_string(),
            tac_tool: PathBuf::from(tac).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
        };
        let (src_guard, src_file) = temporary_file("mp4").unwrap();
        drop(src_file);
        let status = Command::new(&ffmpeg)
            .args([
                "-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=2",
                "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=2",
                "-c:v", "mpeg4", "-c:a", "aac",
            ])
            .arg(&src_guard.0)
            .status()
            .unwrap();
        assert!(status.success());
        let (reserve, f) = temporary_file("tnv3").unwrap();
        drop(f);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        let result = encode_pair(
            Path::new(&ffmpeg),
            &src_guard.0,
            folder,
            name,
            30,
            None,
            AdvancedSettings::default(),
            &dependencies,
            |_| {},
        )
        .unwrap();
        let video = Temporary(PathBuf::from(&result.video));
        assert!(result.audio.is_empty(), "TAC is embedded; there is no sidecar");
        assert!(!video.0.with_extension("mp3").exists());
        let info = tau_video::Tnv3Reader::open(File::open(&video.0).unwrap()).unwrap().info().clone();
        assert_eq!(info.frame_count, 48);
        assert_eq!((info.audio_channels, info.audio_samples), (2, 96_000), "audio fitted to the video");
        assert!(info.flags & 2 != 0);
        let check = Command::new(&tnv3).arg("check").arg(&video.0).output().unwrap();
        assert!(String::from_utf8_lossy(&check.stdout).contains("valid"), "{:?}", check);
        // an existing output is never overwritten
        assert!(
            encode_pair(Path::new(&ffmpeg), &src_guard.0, folder, name, 30, None,
                AdvancedSettings::default(), &dependencies, |_| {})
            .is_err()
        );
    }
    #[test]
    fn tac_audio_without_configured_tools_fails_before_publication() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else { return };
        let (Some(tvc), Some(tnv3)) = (std::env::var_os("TAU_TEST_TVC"), std::env::var_os("TAU_TEST_TNV3")) else { return };
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default() // no TAC tool
        };
        let (src_guard, src_file) = temporary_file("mp4").unwrap();
        drop(src_file);
        assert!(Command::new(&ffmpeg).args([
            "-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=1",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:v", "mpeg4", "-c:a", "aac",
        ]).arg(&src_guard.0).status().unwrap().success());
        let (reserve, f) = temporary_file("tnv3").unwrap();
        drop(f);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let folder = reserve.0.parent().unwrap();
        let error = encode_pair(Path::new(&ffmpeg), &src_guard.0, folder, name, 30, None,
            AdvancedSettings::default(), &dependencies, |_| {}).unwrap_err();
        assert!(error.contains("TAC encoder"), "{error}");
        assert!(!reserve.0.exists());
    }
    #[test]
    fn real_media_pair_and_no_overwrite() {
        let (Some(ffmpeg), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
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
            legacy_mp3(),
            &dependencies,
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
                legacy_mp3(),
                &dependencies,
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
                legacy_mp3(),
                &dependencies,
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
    #[cfg(unix)]
    #[test]
    fn native_ffmpeg_nonzero_exit_publishes_neither_file() {
        let (Some(real_ffmpeg), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let (_src_guard, file) = temporary_file("mp4").unwrap();
        let source = _src_guard.0.clone();
        drop(file);
        assert!(
            Command::new(&real_ffmpeg)
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
                    "aac"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let (wrapper, mut file) = temporary_file("sh").unwrap();
        use std::io::Write;
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "'{}' \"$@\"", PathBuf::from(&real_ffmpeg).display()).unwrap();
        writeln!(file, "exit 23").unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let folder = reserve.0.parent().unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
        };
        let result = encode_pair(
            &wrapper.0,
            &source,
            folder,
            name,
            30,
            None,
            legacy_mp3(),
            &dependencies,
            |_| {},
        );
        assert!(matches!(result, Err(message) if message.contains("Video conversion failed")));
        assert!(!folder.join(format!("{name}.tnv3")).exists());
        assert!(!folder.join(format!("{name}.mp3")).exists());
    }
    #[cfg(feature = "experimental-rust-encoder")]
    #[test]
    fn legacy_mode_rust_keeps_pure_rust_encoder_available() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else {
            return;
        };
        let (_source_guard, file) = temporary_file("mp4").unwrap();
        let source = _source_guard.0.clone();
        drop(file);
        let created = Command::new(&ffmpeg)
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
            .arg(&source)
            .status()
            .unwrap();
        assert!(created.success());
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let result = encode_pair_impl(
            Path::new(&ffmpeg),
            &source,
            reserve.0.parent().unwrap(),
            name,
            30,
            None,
            true,
            legacy_mp3(),
            &Dependencies::default(),
            &JobControl::default(),
            |_| Ok(()),
            |_| {},
        )
        .unwrap();
        assert_eq!(result.frames, 24);
        let video_path = PathBuf::from(&result.video);
        assert_eq!(
            tau_video::Tnv3Reader::open(File::open(&video_path).unwrap())
                .unwrap()
                .info()
                .frame_count,
            24,
            "pure Rust TVC1 must be muxed into a valid TNV3 container"
        );
        let _video = Temporary(video_path);
        let _audio = Temporary(PathBuf::from(result.audio));
    }
    #[test]
    fn native_default_workspace_handles_more_than_one_full_segment() {
        let (Some(ffmpeg), Some(tvc), Some(tnv3)) = (
            std::env::var_os("TAU_TEST_FFMPEG"),
            std::env::var_os("TAU_TEST_TVC"),
            std::env::var_os("TAU_TEST_TNV3"),
        ) else {
            return;
        };
        let (_source_guard, file) = temporary_file("mp4").unwrap();
        let source = _source_guard.0.clone();
        drop(file);
        let created = Command::new(&ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=400x360:rate=24:duration=20.041667",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=20.041667",
                "-c:v",
                "mpeg4",
                "-q:v",
                "4",
                "-c:a",
                "aac",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(created.success());
        let (reserve, file) = temporary_file("tnv3").unwrap();
        drop(file);
        std::fs::remove_file(&reserve.0).unwrap();
        let name = reserve.0.file_stem().unwrap().to_str().unwrap();
        let dependencies = Dependencies {
            tvc_encoder: PathBuf::from(tvc).display().to_string(),
            tnv3_tool: PathBuf::from(tnv3).display().to_string(),
            workspace_limit_bytes: 512u64 * 1024 * 1024,
            ..Default::default()
        };
        let mut video_progress = Vec::new();
        let result = encode_pair(
            Path::new(&ffmpeg),
            &source,
            reserve.0.parent().unwrap(),
            name,
            30,
            None,
            legacy_mp3(),
            &dependencies,
            |event| {
                if event.phase == "video" {
                    video_progress.push(event.frames);
                }
            },
        )
        .unwrap();
        assert_eq!(result.frames, 481);
        assert!(
            video_progress.contains(&480),
            "first segment must report progress: {video_progress:?}"
        );
        assert!(
            video_progress.contains(&481),
            "last segment must report progress: {video_progress:?}"
        );
        let video = Temporary(PathBuf::from(result.video));
        let audio = Temporary(PathBuf::from(result.audio));
        assert_eq!(
            tau_video::Tnv3Reader::open(File::open(&video.0).unwrap())
                .unwrap()
                .info()
                .frame_count,
            481
        );
        assert!(std::fs::metadata(&audio.0).unwrap().len() > 1_000);
    }
}

//! POSIX-only process adapter for ffprobe.
//!
//! Each child starts in a new process group. This module performs nonblocking
//! reads in the caller thread (no reader threads), caps each stream while
//! reading, and kills the group before reaping the leader on failure. It uses
//! `waitid(...WNOWAIT)` to keep the child unreaped while group signaling is
//! attempted. Same-group descendants that hold output pipes after leader exit
//! are targeted; descendants that close pipes or leave the group (for example
//! via `setsid`) are outside the cleanup guarantee.
//!
//! The caller must own SIGCHLD waiting for this child. An observed ECHILD
//! fails closed without a group signal. POSIX has no atomic operation that
//! combines WNOWAIT ownership validation with killpg, so a concurrent external
//! waiter must not reap this child between those calls.
//! POSIX interfaces used: `waitid`/`WNOWAIT` (https://pubs.opengroup.org/onlinepubs/9799919799/functions/waitid.html),
//! `setpgid` (https://pubs.opengroup.org/onlinepubs/9799919799/functions/setpgid.html),
//! and `kill` process-group signaling (https://pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html).

use crate::probe::{
    MAX_PROBE_JSON_BYTES, MAX_PROBE_STDERR_BYTES, ProbeError, ProbeInfo, parse_ffprobe_json,
};
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(2);
const TERMINATE_GRACE: Duration = Duration::from_millis(250);
const INHERITED_PIPE_GRACE: Duration = Duration::from_millis(100);
const MAX_GEOMETRY_Y4M_BYTES: usize = 1024 * 1024; // one 800x720 4:2:0 frame is 864,000 bytes
const MAX_SAMPLE_Y4M_BYTES: usize = 48 * 1024 * 1024; // 48 frames of 800x720
const MAX_SAMPLE_TVC_BYTES: u64 = 16 * 1024 * 1024;
const MAX_VERSION_OUTPUT_BYTES: usize = 16 * 1024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_SAMPLE_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run a selected tool's version switch with process-group supervision and
/// bounded stdout/stderr. This reports only the executable's own response; it
/// does not establish compatibility or bind the result to a later execution.
pub(super) fn run_version_command(
    executable: &Path,
    args: &[&str],
) -> Result<(Option<i32>, Vec<u8>, Vec<u8>), String> {
    run_version_command_with_timeout(executable, args, VERSION_TIMEOUT)
}

fn run_version_command_with_timeout(
    executable: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(Option<i32>, Vec<u8>, Vec<u8>), String> {
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not start version query: {e}"))?;
    let pgid = rustix::process::Pid::from_child(&child);
    if let Err(error) = verify_process_group(&child, pgid) {
        return terminate_direct_child_bounded::<(Option<i32>, Vec<u8>, Vec<u8>)>(
            &mut child, error,
        )
        .map_err(|e| format!("version query cleanup failed: {e}"));
    }
    let Some(mut stdout) = child.stdout.take() else {
        return Err(format!(
            "version stdout pipe missing; cleanup: {:?}",
            kill_group_and_reap(&mut child, pgid)
        ));
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err(format!(
            "version stderr pipe missing; cleanup: {:?}",
            kill_group_and_reap(&mut child, pgid)
        ));
    };
    if let Err(e) = set_nonblocking(&stdout) {
        return Err(format!(
            "could not configure version stdout: {e}; cleanup: {:?}",
            kill_group_and_reap(&mut child, pgid)
        ));
    }
    if let Err(e) = set_nonblocking(&stderr) {
        return Err(format!(
            "could not configure version stderr: {e}; cleanup: {:?}",
            kill_group_and_reap(&mut child, pgid)
        ));
    }
    let mut out = Vec::with_capacity(1024);
    let mut err = Vec::with_capacity(1024);
    let (mut out_eof, mut err_eof) = (false, false);
    let mut exited_at = None;
    let deadline = Instant::now() + timeout;
    loop {
        if let Err(e) = drain(
            &mut stdout,
            &mut out,
            MAX_VERSION_OUTPUT_BYTES,
            "stdout",
            &mut out_eof,
        ) {
            let cleanup = kill_group_and_reap(&mut child, pgid);
            return Err(format!(
                "version query output rejected: {e}; cleanup: {cleanup:?}"
            ));
        }
        if let Err(e) = drain(
            &mut stderr,
            &mut err,
            MAX_VERSION_OUTPUT_BYTES,
            "stderr",
            &mut err_eof,
        ) {
            let cleanup = kill_group_and_reap(&mut child, pgid);
            return Err(format!(
                "version query output rejected: {e}; cleanup: {cleanup:?}"
            ));
        }
        if exited_at.is_none() {
            match child_exited_without_reaping(&child) {
                Ok(true) => exited_at = Some(Instant::now()),
                Ok(false) if Instant::now() >= deadline => {
                    let cleanup = kill_group_and_reap(&mut child, pgid);
                    return Err(format!("version query timed out; cleanup: {cleanup:?}"));
                }
                Ok(false) => {}
                Err(e) => return Err(format!("version query child status is unverified: {e}")),
            }
        }
        if let Some(at) = exited_at {
            if out_eof && err_eof {
                let status = child
                    .wait()
                    .map_err(|e| format!("could not reap version query: {e}"))?;
                return Ok((status.code(), out, err));
            }
            if at.elapsed() >= INHERITED_PIPE_GRACE {
                let cleanup = kill_group_and_reap(&mut child, pgid);
                return Err(format!(
                    "version query left inherited output pipes open; cleanup: {cleanup:?}"
                ));
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Execute the production FFmpeg filter for one frame, with the same bounded
/// process-group supervision as ffprobe. A 400x360 YUV420 frame fits the cap.
pub(super) fn run_geometry(
    ffmpeg: &Path,
    input: &Path,
    timeout: Duration,
    profile: crate::probe::Profile,
) -> Result<Vec<u8>, ProbeError> {
    run_ffmpeg_y4m(ffmpeg, input, timeout, 1, MAX_GEOMETRY_Y4M_BYTES, profile)
}

pub(super) fn run_sample_frames(
    ffmpeg: &Path,
    input: &Path,
    frames: u32,
    timeout: Duration,
    profile: crate::probe::Profile,
) -> Result<Vec<u8>, ProbeError> {
    run_ffmpeg_y4m(ffmpeg, input, timeout, frames, MAX_SAMPLE_Y4M_BYTES, profile)
}

fn run_ffmpeg_y4m(
    ffmpeg: &Path,
    input: &Path,
    timeout: Duration,
    frames: u32,
    max_bytes: usize,
    profile: crate::probe::Profile,
) -> Result<Vec<u8>, ProbeError> {
    let frame_count = frames.to_string();
    let filter = profile.filter();
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-vf",
            filter.as_str(),
            "-frames:v",
            frame_count.as_str(),
            "-pix_fmt",
            "yuv420p",
            "-f",
            "yuv4mpegpipe",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| ProbeError::Spawn(e.to_string()))?;
    let pgid = rustix::process::Pid::from_child(&child);
    if let Err(error) = verify_process_group(&child, pgid) {
        return match terminate_direct_child_bounded::<Vec<u8>>(&mut child, error) {
            Err(e) => Err(e),
            Ok(_) => unreachable!(),
        };
    }
    let Some(mut stdout) = child.stdout.take() else {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait("stdout pipe missing".into()));
    };
    let Some(mut stderr) = child.stderr.take() else {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait("stderr pipe missing".into()));
    };
    if let Err(e) = set_nonblocking(&stdout) {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait(e.to_string()));
    }
    if let Err(e) = set_nonblocking(&stderr) {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait(e.to_string()));
    }
    let mut out = Vec::with_capacity(4096);
    let mut err = Vec::with_capacity(1024);
    let (mut out_eof, mut err_eof) = (false, false);
    let mut child_exited_at = None;
    let mut group_kill_at = None;
    let deadline = Instant::now() + timeout;
    loop {
        if let Err(e) = drain(&mut stdout, &mut out, max_bytes, "stdout", &mut out_eof) {
            kill_group_and_reap(&mut child, pgid)?;
            return Err(e);
        }
        if let Err(e) = drain(
            &mut stderr,
            &mut err,
            MAX_PROBE_STDERR_BYTES,
            "stderr",
            &mut err_eof,
        ) {
            kill_group_and_reap(&mut child, pgid)?;
            return Err(e);
        }
        if child_exited_at.is_none() {
            match child_exited_without_reaping(&child) {
                Err(e) => return Err(e),
                Ok(true) => child_exited_at = Some(Instant::now()),
                Ok(false) if Instant::now() >= deadline => {
                    kill_group_and_reap(&mut child, pgid)?;
                    return Err(ProbeError::TimedOut);
                }
                Ok(false) => {}
            }
        }
        if let Some(exited_at) = child_exited_at {
            if out_eof && err_eof {
                let status = child.wait().map_err(|e| ProbeError::Wait(e.to_string()))?;
                if status.success() {
                    return Ok(out);
                }
                return Err(ProbeError::ToolFailed {
                    status: status.code(),
                    stderr: String::from_utf8_lossy(&err).into_owned(),
                    stderr_truncated: false,
                });
            }
            if group_kill_at.is_none() && exited_at.elapsed() >= INHERITED_PIPE_GRACE {
                let _ = child_exited_without_reaping(&child)?;
                if let Err(error) = signal_group(pgid) {
                    let _ = child.wait();
                    return Err(error);
                }
                group_kill_at = Some(Instant::now());
            } else if group_kill_at.is_some_and(|t| t.elapsed() >= TERMINATE_GRACE) {
                let _ = child_exited_without_reaping(&child)?;
                let _ = child.wait();
                return Err(ProbeError::DescendantPipeLeak);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Encode one bounded Y4M sample with the selected production TVC executable.
/// The private directory is removed on every verified process outcome.
pub(super) fn encode_tvc_sample(
    encoder: &Path,
    y4m: &[u8],
    level: u8,
    timeout: Duration,
) -> Result<u64, ProbeError> {
    let directory = create_sample_directory()?;
    let mut cleanup = SampleDirectory {
        path: directory,
        retained: false,
    };
    let result = encode_tvc_sample_inner(encoder, y4m, level, timeout, &mut cleanup);
    if !cleanup.retained {
        if let Err(error) = std::fs::remove_dir_all(&cleanup.path) {
            return Err(match result {
                Ok(_) => ProbeError::Wait(format!(
                    "could not remove sample workspace '{}': {error}",
                    cleanup.path.display()
                )),
                Err(prior) => ProbeError::Wait(format!(
                    "{prior}; could not remove sample workspace '{}': {error}",
                    cleanup.path.display()
                )),
            });
        }
    }
    result
}

fn encode_tvc_sample_inner(
    encoder: &Path,
    y4m: &[u8],
    level: u8,
    timeout: Duration,
    cleanup: &mut SampleDirectory,
) -> Result<u64, ProbeError> {
    let directory = cleanup.path.clone();
    let input = directory.join("sample.y4m");
    let output = directory.join("sample.tvc");
    std::fs::write(&input, y4m)
        .map_err(|e| ProbeError::Wait(format!("could not stage sample Y4M: {e}")))?;
    let mut child = Command::new(encoder)
        .arg(&input)
        .arg(&output)
        .args(["-q", "30", "-l"])
        .arg(level.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| ProbeError::Spawn(format!("could not start TVC sample encoder: {e}")))?;
    let pgid = rustix::process::Pid::from_child(&child);
    if let Err(error) = verify_process_group(&child, pgid) {
        return match terminate_direct_child_bounded::<u64>(&mut child, error) {
            Err(error @ ProbeError::CleanupUnconfirmed(_)) => {
                Err(retain_sample_if_uncertain(cleanup, error))
            }
            Err(error) => Err(error),
            Ok(_) => unreachable!(),
        };
    }
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        if let Err(error) = kill_group_and_reap(&mut child, pgid) {
            return Err(retain_sample_if_uncertain(cleanup, error));
        }
        return Err(ProbeError::Wait(
            "TVC sample encoder pipe capture failed".into(),
        ));
    };
    if let Err(error) = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr)) {
        if let Err(cleanup_error) = kill_group_and_reap(&mut child, pgid) {
            return Err(retain_sample_if_uncertain(cleanup, cleanup_error));
        }
        return Err(ProbeError::Wait(error.to_string()));
    }
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let (mut stdout_eof, mut stderr_eof) = (false, false);
    let deadline = Instant::now() + timeout;
    let mut child_exited_at = None;
    let mut group_kill_at = None;
    loop {
        if let Err(error) = drain(
            &mut stdout,
            &mut stdout_bytes,
            4096,
            "stdout",
            &mut stdout_eof,
        )
        .and_then(|()| {
            drain(
                &mut stderr,
                &mut stderr_bytes,
                MAX_PROBE_STDERR_BYTES,
                "stderr",
                &mut stderr_eof,
            )
        }) {
            if let Err(cleanup_error) = kill_sample_and_reap(
                &mut child,
                pgid,
                &mut stdout,
                &mut stderr,
                &mut stdout_eof,
                &mut stderr_eof,
            ) {
                return Err(retain_sample_if_uncertain(cleanup, cleanup_error));
            }
            return Err(error);
        }
        if std::fs::metadata(&output).is_ok_and(|metadata| metadata.len() > MAX_SAMPLE_TVC_BYTES) {
            if let Err(error) = kill_sample_and_reap(
                &mut child,
                pgid,
                &mut stdout,
                &mut stderr,
                &mut stdout_eof,
                &mut stderr_eof,
            ) {
                return Err(retain_sample_if_uncertain(cleanup, error));
            }
            return Err(ProbeError::OutputTooLarge {
                stream: "TVC sample",
                limit: MAX_SAMPLE_TVC_BYTES as usize,
            });
        }
        if child_exited_at.is_none() {
            match child_exited_without_reaping(&child) {
                Err(error) => return Err(retain_sample_if_uncertain(cleanup, error)),
                Ok(true) => child_exited_at = Some(Instant::now()),
                Ok(false) if Instant::now() >= deadline => {
                    if let Err(error) = kill_sample_and_reap(
                        &mut child,
                        pgid,
                        &mut stdout,
                        &mut stderr,
                        &mut stdout_eof,
                        &mut stderr_eof,
                    ) {
                        return Err(retain_sample_if_uncertain(cleanup, error));
                    }
                    return Err(ProbeError::TimedOut);
                }
                Ok(false) => {}
            }
        }
        if let Some(exited_at) = child_exited_at {
            if stdout_eof && stderr_eof {
                let status = child.wait().map_err(|e| {
                    retain_sample_if_uncertain(
                        cleanup,
                        ProbeError::CleanupUnconfirmed(format!(
                            "could not reap TVC sample encoder: {e}"
                        )),
                    )
                })?;
                if !status.success() {
                    return Err(ProbeError::ToolFailed {
                        status: status.code(),
                        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
                        stderr_truncated: false,
                    });
                }
                let metadata = std::fs::symlink_metadata(&output)
                    .map_err(|e| ProbeError::Wait(format!("TVC sample output is missing: {e}")))?;
                if !metadata.file_type().is_file() {
                    return Err(ProbeError::InvalidShapeWithContext(
                        "TVC sample output is not a regular file".into(),
                    ));
                }
                let bytes = metadata.len();
                if bytes > MAX_SAMPLE_TVC_BYTES {
                    return Err(ProbeError::OutputTooLarge {
                        stream: "TVC sample",
                        limit: MAX_SAMPLE_TVC_BYTES as usize,
                    });
                }
                if bytes == 0 {
                    return Err(ProbeError::InvalidShapeWithContext(
                        "TVC sample encoder produced an empty output".into(),
                    ));
                }
                return Ok(bytes);
            }
            if group_kill_at.is_none() && exited_at.elapsed() >= INHERITED_PIPE_GRACE {
                if let Err(error) = child_exited_without_reaping(&child) {
                    return Err(retain_sample_if_uncertain(cleanup, error));
                }
                if let Err(error) = signal_group(pgid) {
                    let _ = child.wait();
                    return Err(retain_sample_if_uncertain(cleanup, error));
                }
                group_kill_at = Some(Instant::now());
            } else if group_kill_at.is_some_and(|at| at.elapsed() >= TERMINATE_GRACE) {
                let _ = child_exited_without_reaping(&child);
                let _ = child.wait();
                return Err(retain_sample_if_uncertain(
                    cleanup,
                    ProbeError::DescendantPipeLeak,
                ));
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn kill_sample_and_reap(
    child: &mut Child,
    pgid: rustix::process::Pid,
    stdout: &mut impl Read,
    stderr: &mut impl Read,
    stdout_eof: &mut bool,
    stderr_eof: &mut bool,
) -> Result<(), ProbeError> {
    kill_group_and_reap(child, pgid)?;
    let deadline = Instant::now() + TERMINATE_GRACE;
    let mut scratch = [0u8; 8192];
    while !(*stdout_eof && *stderr_eof) && Instant::now() < deadline {
        drain_discard(stdout, stdout_eof, &mut scratch)?;
        drain_discard(stderr, stderr_eof, &mut scratch)?;
        if !(*stdout_eof && *stderr_eof) {
            thread::sleep(POLL_INTERVAL);
        }
    }
    if *stdout_eof && *stderr_eof {
        Ok(())
    } else {
        Err(ProbeError::DescendantPipeLeak)
    }
}

fn drain_discard<R: Read>(
    pipe: &mut R,
    eof: &mut bool,
    scratch: &mut [u8],
) -> Result<(), ProbeError> {
    loop {
        match pipe.read(scratch) {
            Ok(0) => {
                *eof = true;
                return Ok(());
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ProbeError::Wait(error.to_string())),
        }
    }
}

struct SampleDirectory {
    path: std::path::PathBuf,
    retained: bool,
}
impl Drop for SampleDirectory {
    fn drop(&mut self) {
        if !self.retained {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn retain_sample_if_uncertain(cleanup: &mut SampleDirectory, error: ProbeError) -> ProbeError {
    if !matches!(
        &error,
        ProbeError::CleanupUnconfirmed(_)
            | ProbeError::OwnershipLost(_)
            | ProbeError::GroupSignalFailed(_)
            | ProbeError::KillUnconfirmed
            | ProbeError::Wait(_)
            | ProbeError::DescendantPipeLeak
    ) {
        return error;
    }
    cleanup.retained = true;
    ProbeError::CleanupUnconfirmed(format!(
        "{error}; sample staging retained at '{}'",
        cleanup.path.display()
    ))
}

fn create_sample_directory() -> Result<std::path::PathBuf, ProbeError> {
    for _ in 0..100 {
        let id = NEXT_SAMPLE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tau-probe-sample-{}-{id}", std::process::id()));
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&path)
        };
        #[cfg(not(unix))]
        let created = std::fs::create_dir(&path);
        match created {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ProbeError::Wait(format!(
                    "could not create sample workspace: {error}"
                )));
            }
        }
    }
    Err(ProbeError::Wait(
        "could not reserve unique sample workspace".into(),
    ))
}

pub(super) fn run_probe_with_timeout(
    ffprobe: &Path,
    input: &Path,
    timeout: Duration,
) -> Result<ProbeInfo, ProbeError> {
    let mut child = Command::new(ffprobe)
        .args([
            "-v", "error", "-show_format", "-show_streams", "-of", "json",
            "-show_entries",
            "format=format_name,duration,bit_rate:stream=index,codec_type,codec_name,width,height,avg_frame_rate,r_frame_rate,duration,pix_fmt,sample_aspect_ratio,display_aspect_ratio,sample_rate,channels,bit_rate:stream_tags=language,title:stream_side_data=rotation",
            "-i",
        ])
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| ProbeError::Spawn(e.to_string()))?;
    let pgid = rustix::process::Pid::from_child(&child);
    if let Err(error) = verify_process_group(&child, pgid) {
        return terminate_direct_child_bounded(&mut child, error);
    }
    let Some(mut stdout) = child.stdout.take() else {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait("stdout pipe missing".into()));
    };
    let Some(mut stderr) = child.stderr.take() else {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait("stderr pipe missing".into()));
    };
    if let Err(e) = set_nonblocking(&stdout) {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait(e.to_string()));
    }
    if let Err(e) = set_nonblocking(&stderr) {
        kill_group_and_reap(&mut child, pgid)?;
        return Err(ProbeError::Wait(e.to_string()));
    }

    let mut out = Vec::with_capacity(8192);
    let mut err = Vec::with_capacity(4096);
    let mut out_eof = false;
    let mut err_eof = false;
    let mut child_exited_at = None;
    let mut group_kill_at = None;
    let deadline = Instant::now() + timeout;

    loop {
        if let Err(e) = drain(
            &mut stdout,
            &mut out,
            MAX_PROBE_JSON_BYTES,
            "stdout",
            &mut out_eof,
        ) {
            kill_group_and_reap(&mut child, pgid)?;
            return Err(e);
        }
        if let Err(e) = drain(
            &mut stderr,
            &mut err,
            MAX_PROBE_STDERR_BYTES,
            "stderr",
            &mut err_eof,
        ) {
            kill_group_and_reap(&mut child, pgid)?;
            return Err(e);
        }

        if child_exited_at.is_none() {
            match child_exited_without_reaping(&child) {
                Err(e) => return Err(e),
                Ok(true) => child_exited_at = Some(Instant::now()),
                Ok(false) if Instant::now() >= deadline => {
                    kill_group_and_reap(&mut child, pgid)?;
                    return Err(ProbeError::TimedOut);
                }
                Ok(false) => {}
            }
        }

        if let Some(exited_at) = child_exited_at {
            if out_eof && err_eof {
                // All writers closed the pipes. Reap the confirmed zombie;
                // do not signal a process group after its leader's normal exit.
                let status = child.wait().map_err(|e| ProbeError::Wait(e.to_string()))?;
                return finish(status, out, err);
            }
            if group_kill_at.is_none() && exited_at.elapsed() >= INHERITED_PIPE_GRACE {
                // Leader remains a zombie, so this numeric PGID cannot be
                // recycled before we signal it and then reap the leader.
                let _ = child_exited_without_reaping(&child)?;
                if let Err(error) = signal_group(pgid) {
                    let _ = child.wait(); // leader is a confirmed zombie.
                    return Err(error);
                }
                group_kill_at = Some(Instant::now());
            } else if group_kill_at.is_some_and(|t| t.elapsed() >= TERMINATE_GRACE) {
                // waitid confirmed this leader is already a zombie, so
                // this wait cannot block; reap it before returning.
                let _ = child_exited_without_reaping(&child)?;
                let _ = child.wait();
                return Err(ProbeError::DescendantPipeLeak);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn finish(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Result<ProbeInfo, ProbeError> {
    let stderr = String::from_utf8_lossy(&stderr).into_owned();
    if !status.success() {
        return Err(ProbeError::ToolFailed {
            status: status.code(),
            stderr,
            stderr_truncated: false,
        });
    }
    parse_ffprobe_json(&stdout)
}

fn set_nonblocking<T: std::os::fd::AsFd>(pipe: &T) -> rustix::io::Result<()> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let flags = fcntl_getfl(pipe.as_fd())?;
    fcntl_setfl(pipe.as_fd(), flags | OFlags::NONBLOCK)
}

fn drain<R: Read>(
    pipe: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
    stream: &'static str,
    eof: &mut bool,
) -> Result<(), ProbeError> {
    let mut scratch = [0u8; 8192];
    loop {
        let room = limit.saturating_sub(output.len());
        let read_len = scratch.len().min(room.saturating_add(1));
        match pipe.read(&mut scratch[..read_len]) {
            Ok(0) => {
                *eof = true;
                return Ok(());
            }
            Ok(n) if n > room => return Err(ProbeError::OutputTooLarge { stream, limit }),
            Ok(n) => output.extend_from_slice(&scratch[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ProbeError::Wait(e.to_string())),
        }
    }
}

fn child_exited_without_reaping(child: &Child) -> Result<bool, ProbeError> {
    use rustix::process::{WaitId, WaitIdOptions, waitid};
    loop {
        match waitid(
            WaitId::Pid(rustix::process::Pid::from_child(child)),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => return Ok(status.is_some()),
            Err(e) if e == rustix::io::Errno::INTR => continue,
            Err(e) if e == rustix::io::Errno::CHILD => {
                return Err(ProbeError::OwnershipLost(e.to_string()));
            }
            Err(e) => {
                return Err(ProbeError::CleanupUnconfirmed(format!(
                    "waitid could not establish whether ffprobe exited; child may still be running: {e}"
                )));
            }
        }
    }
}

fn kill_group_and_reap(child: &mut Child, pgid: rustix::process::Pid) -> Result<(), ProbeError> {
    // Do not signal a numeric PGID unless a nonblocking wait confirms this
    // child is still ours or is a waitable zombie. ECHILD fails closed.
    let _ = child_exited_without_reaping(child)?;
    let signal = signal_group(pgid);
    if !matches!(signal, Ok(true)) {
        if child_exited_without_reaping(child)? {
            let _ = child.wait(); // Reap only after independently confirming a zombie.
        }
        return match signal {
            Err(error) => Err(error),
            Ok(false) => Err(ProbeError::GroupSignalFailed(
                "ESRCH: process group is absent; cleanup not confirmed".into(),
            )),
            Ok(true) => unreachable!(),
        };
    }
    let deadline = Instant::now() + TERMINATE_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) => return Err(ProbeError::KillUnconfirmed),
            Err(e) => return Err(ProbeError::OwnershipLost(e.to_string())),
        }
    }
}

fn verify_process_group(child: &Child, expected: rustix::process::Pid) -> Result<(), ProbeError> {
    let actual = rustix::process::getpgid(Some(rustix::process::Pid::from_child(child)))
        .map_err(|e| ProbeError::GroupSetupFailed(e.to_string()))?;
    if actual != expected {
        return Err(ProbeError::GroupSetupFailed(format!(
            "expected {expected:?}, child belongs to {actual:?}"
        )));
    }
    Ok(())
}

/// Clean up setup failures using only the owned direct-child handle. A live
/// child cannot yet have been reaped by another waitpid caller, so Child::kill
/// is safe after WNOWAIT reports no exit. Never signal a group on this path.
fn terminate_direct_child_bounded<T>(
    child: &mut Child,
    cause: ProbeError,
) -> Result<T, ProbeError> {
    match child_exited_without_reaping(child) {
        Ok(true) => {
            let _ = child.wait();
            Err(cause)
        }
        Ok(false) => {
            if let Err(error) = child.kill() {
                return Err(ProbeError::CleanupUnconfirmed(format!(
                    "{cause}; direct-child kill failed: {error}"
                )));
            }
            let deadline = Instant::now() + TERMINATE_GRACE;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return Err(cause),
                    Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
                    Ok(None) => {
                        return Err(ProbeError::CleanupUnconfirmed(format!(
                            "{cause}; direct-child termination was not confirmed"
                        )));
                    }
                    Err(error) => {
                        return Err(ProbeError::CleanupUnconfirmed(format!(
                            "{cause}; direct-child reap failed: {error}"
                        )));
                    }
                }
            }
        }
        Err(error) => Err(ProbeError::CleanupUnconfirmed(format!(
            "{cause}; child ownership could not be verified: {error}"
        ))),
    }
}

/// ESRCH means there is no process group left to signal; callers may proceed
/// only when their surrounding state makes that safe. Other failures do not
/// imply cleanup and are returned to the caller.
fn signal_group(pgid: rustix::process::Pid) -> Result<bool, ProbeError> {
    match rustix::process::kill_process_group(pgid, rustix::process::Signal::KILL) {
        Ok(()) => Ok(true),
        Err(e) if e == rustix::io::Errno::SRCH => Ok(false),
        Err(e) => Err(ProbeError::GroupSignalFailed(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    #[test]
    fn version_query_times_out_and_terminates_its_process_group() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::unique_test_dir("tau-version-timeout");
        let script = dir.join("slow-tool");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        let error =
            run_version_command_with_timeout(&script, &["--version"], Duration::from_millis(100))
                .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(error.contains("cleanup: Ok"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_query_stops_same_group_descendant_holding_pipes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::unique_test_dir("tau-version-descendant");
        let script = dir.join("forking-tool");
        std::fs::write(&script, "#!/bin/sh\nsleep 30 &\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        let error =
            run_version_command_with_timeout(&script, &["--version"], Duration::from_secs(2))
                .unwrap_err();
        assert!(error.contains("inherited output pipes"), "{error}");
        assert!(error.contains("cleanup: Ok"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mismatched_group_uses_bounded_direct_child_cleanup() {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = rustix::process::Pid::from_child(&child);
        let wrong = rustix::process::Pid::from_raw(pid.as_raw_pid() + 1).unwrap();
        let cause = verify_process_group(&child, wrong).unwrap_err();
        let start = Instant::now();
        assert!(matches!(
            terminate_direct_child_bounded::<()>(&mut child, cause),
            Err(ProbeError::GroupSetupFailed(_))
        ));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(matches!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG),
            Err(e) if e == rustix::io::Errno::CHILD
        ));
    }

    #[test]
    fn exited_child_is_reaped_when_group_inspection_fails() {
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = rustix::process::Pid::from_child(&child);
        while !child_exited_without_reaping(&child).unwrap() {
            thread::sleep(POLL_INTERVAL);
        }
        let cause = verify_process_group(
            &child,
            rustix::process::Pid::from_raw(pid.as_raw_pid() + 1).unwrap(),
        )
        .unwrap_err();
        assert!(matches!(
            terminate_direct_child_bounded::<()>(&mut child, cause),
            Err(ProbeError::GroupSetupFailed(_))
        ));
        assert!(matches!(
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG),
            Err(e) if e == rustix::io::Errno::CHILD
        ));
    }

    #[test]
    fn external_reaper_is_detected_and_fails_closed_before_group_signal() {
        let mut child = Command::new("sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pgid = rustix::process::Pid::from_child(&child);
        // Simulate another SIGCHLD waiter in the host process reaping first.
        let _ = rustix::process::waitpid(
            Some(rustix::process::Pid::from_child(&child)),
            rustix::process::WaitOptions::empty(),
        )
        .unwrap();
        assert!(matches!(
            kill_group_and_reap(&mut child, pgid),
            Err(ProbeError::OwnershipLost(_))
        ));
    }

    #[test]
    fn sample_encoder_timeout_kills_group_and_removes_verified_staging() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("tau-probe-sleeper-{}", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let prefix = format!("tau-probe-sample-{}-", std::process::id());
        let before = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.file_name())
            .collect::<std::collections::HashSet<_>>();
        let started = Instant::now();
        let error = encode_tvc_sample(&path, b"sample", 1, Duration::from_millis(80)).unwrap_err();
        assert!(matches!(error, ProbeError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(3));
        let after = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.file_name())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            before, after,
            "verified timeout should clean sample staging"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn uncertain_cleanup_retains_sample_staging_and_reports_location() {
        let directory = create_sample_directory().unwrap();
        let guard = SampleDirectory {
            path: directory.clone(),
            retained: false,
        };
        let mut guard = guard;
        let error = retain_sample_if_uncertain(&mut guard, ProbeError::KillUnconfirmed);
        assert!(
            error
                .to_string()
                .contains(directory.to_string_lossy().as_ref())
        );
        drop(guard);
        assert!(directory.is_dir());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

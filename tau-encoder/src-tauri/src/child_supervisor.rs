//! Direct-child supervisor experiment. Not integrated or compiled.
//! Only trusted leaf executables are covered. No external reaper may wait for
//! this child. A process crash or an executable that spawns descendants requires
//! platform-specific containment beyond this prototype.
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct SourceChild {
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
    pub stop: Sender<()>,
    pub finished: JoinHandle<io::Result<ExitStatus>>,
}

/// Keep the Child alive on a private waiter if the active supervisor cannot
/// prove exit. The caller gets an explicit unverified-cleanup error while this
/// owner continues retrying; a failed thread creation falls back to waiting in
/// the current thread so the Child is never simply dropped while live.
fn retain_for_background_reap(child: Child, reason: &str) -> io::Error {
    let owner = Arc::new(Mutex::new(Some(child)));
    let background_owner = owner.clone();
    match thread::Builder::new().name("tau-source-reaper".into()).spawn(move || {
        let mut child = match background_owner.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(ref mut child) = child {
            loop {
                if child.wait().is_ok() {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }) {
        Ok(_detached_owner) => io::Error::other(format!(
            "{reason}; cleanup is unverified; a background waiter retains the child"
        )),
        Err(spawn_error) => {
            let mut child = match owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            if let Some(ref mut child) = child {
                loop {
                    if child.wait().is_ok() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            io::Error::other(format!(
                "{reason}; background reaper creation failed ({spawn_error}); waited in caller"
            ))
        }
    }
}

fn stop_and_reap(mut child: Child) -> io::Result<ExitStatus> {
    // The child handle is owned and reaped only by this supervisor thread.
    // External reapers invalidate that contract and are not supported here.
    match child.try_wait() {
        Ok(Some(status)) => return Ok(status),
        Ok(None) => {}
        Err(error) => return Err(retain_for_background_reap(child, &format!("could not inspect child: {error}"))),
    }
    if let Err(error) = child.kill() {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => return Err(retain_for_background_reap(child, &format!("could not kill child: {error}"))),
            Err(wait_error) => return Err(retain_for_background_reap(child, &format!("kill failed ({error}) and child inspection failed ({wait_error})"))),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut inspect_failures = 0;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => inspect_failures = 0,
            Err(error) => {
                inspect_failures += 1;
                if inspect_failures >= 3 {
                    return Err(retain_for_background_reap(child, &format!("could not inspect child after stop: {error}")));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(retain_for_background_reap(child, "child did not exit after stop timeout"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Spawn an FFmpeg-style source with both output pipes owned by the caller.
/// Dropping all stop senders requests cancellation. The caller must still
/// serialize any Stop request against final output publication.
pub fn spawn_source(command: &mut Command) -> io::Result<SourceChild> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        _ => {
            return match stop_and_reap(child) {
                Ok(_) => Err(io::Error::other("source pipe capture failed")),
                Err(error) => Err(io::Error::other(format!("source pipe capture failed; cleanup unverified: {error}"))),
            };
        }
    };
    let (stop, receiver) = mpsc::channel();
    // Keep ownership outside the closure until the OS confirms thread creation.
    let owner = Arc::new(Mutex::new(Some(child)));
    let worker_owner = owner.clone();
    let spawned = thread::Builder::new().name("tau-source-supervisor".into()).spawn(move || {
        let mut child = match worker_owner.lock() {
            Ok(mut slot) => slot.take().expect("source child ownership"),
            Err(poisoned) => poisoned.into_inner().take().expect("source child ownership"),
        };
        loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                return Err(retain_for_background_reap(child, &format!("could not inspect source child: {error}")));
            }
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return stop_and_reap(child),
            Err(RecvTimeoutError::Timeout) => {}
        }
        }
    });
    let finished = match spawned {
        Ok(handle) => handle,
        Err(error) => {
            let child = match owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            }.ok_or_else(|| io::Error::other("source supervisor failed; child ownership unverified"))?;
            return match stop_and_reap(child) {
                Ok(_) => Err(io::Error::other(format!("could not start source supervisor: {error}"))),
                Err(cleanup) => Err(io::Error::other(format!("could not start source supervisor: {error}; cleanup unverified: {cleanup}"))),
            };
        }
    };
    Ok(SourceChild { stdout, stderr, stop, finished })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reaps_a_real_ffmpeg_source_when_configured() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else { return };
        let mut command = Command::new(ffmpeg);
        command.args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", "anullsrc", "-f", "null", "-"]);
        let child = spawn_source(&mut command).unwrap();
        child.stop.send(()).unwrap();
        let status = child.finished.join().unwrap().unwrap();
        assert!(!status.success());
    }
}

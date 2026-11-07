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

/// Make a bounded best effort to reap a Child if the active supervisor cannot
/// prove exit. Interrupted waits retry only until the deadline; other errors
/// (including ECHILD) stop immediately. If thread creation fails, return
/// without waiting synchronously. The bounded reaper can drop a still-live
/// Child when it times out or hits a permanent error, so eventual reaping is
/// not guaranteed and cleanup remains unverified.
fn retain_for_background_reap(child: Child, reason: &str) -> io::Error {
    let owner = Arc::new(Mutex::new(Some(child)));
    let background_owner = owner.clone();
    match thread::Builder::new()
        .name("tau-source-reaper".into())
        .spawn(move || {
            let child = match background_owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            if let Some(mut child) = child {
                let _ = bounded_reap_child(&mut child, Duration::from_secs(3));
            }
        }) {
        Ok(_detached_owner) => io::Error::other(format!(
            "{reason}; cleanup is unverified; bounded background reaping was requested"
        )),
        Err(spawn_error) => {
            let child = match owner.lock() {
                Ok(mut slot) => slot.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            failed_reaper_spawn(child, reason, &spawn_error)
        }
    }
}

fn failed_reaper_spawn<T>(child: Option<T>, reason: &str, spawn_error: &io::Error) -> io::Error {
    drop(child);
    io::Error::other(format!(
        "{reason}; background reaper creation failed ({spawn_error}); cleanup is unverified; no synchronous wait was attempted"
    ))
}

fn bounded_reap_child(child: &mut Child, timeout: Duration) -> io::Result<()> {
    bounded_reap_with(|| child.try_wait(), timeout)
}

fn bounded_reap_with(
    mut poll: impl FnMut() -> io::Result<Option<ExitStatus>>,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match poll() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child did not exit during bounded reap window",
                ));
            }
            Err(error)
                if error.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "permanent or unclassified child wait error; stopping retries: {error}"
                    ),
                ));
            }
        }
    }
}

fn stop_and_reap(mut child: Child) -> io::Result<ExitStatus> {
    // The child handle is owned and reaped only by this supervisor thread.
    // External reapers invalidate that contract and are not supported here.
    match child.try_wait() {
        Ok(Some(status)) => return Ok(status),
        Ok(None) => {}
        Err(error) => {
            return Err(retain_for_background_reap(
                child,
                &format!("could not inspect child: {error}"),
            ));
        }
    }
    if let Err(error) = child.kill() {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                return Err(retain_for_background_reap(
                    child,
                    &format!("could not kill child: {error}"),
                ));
            }
            Err(wait_error) => {
                return Err(retain_for_background_reap(
                    child,
                    &format!("kill failed ({error}) and child inspection failed ({wait_error})"),
                ));
            }
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
                    return Err(retain_for_background_reap(
                        child,
                        &format!("could not inspect child after stop: {error}"),
                    ));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(retain_for_background_reap(
                child,
                "child did not exit after stop timeout",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Spawn an FFmpeg-style source with both output pipes owned by the caller.
/// Dropping all stop senders requests cancellation. The caller must still
/// serialize any Stop request against final output publication.
pub fn spawn_source(command: &mut Command) -> io::Result<SourceChild> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        _ => {
            return match stop_and_reap(child) {
                Ok(_) => Err(io::Error::other("source pipe capture failed")),
                Err(error) => Err(io::Error::other(format!(
                    "source pipe capture failed; cleanup unverified: {error}"
                ))),
            };
        }
    };
    let (stop, receiver) = mpsc::channel();
    // Keep ownership outside the closure until the OS confirms thread creation.
    let owner = Arc::new(Mutex::new(Some(child)));
    let worker_owner = owner.clone();
    let spawned = thread::Builder::new()
        .name("tau-source-supervisor".into())
        .spawn(move || {
            let mut child = match worker_owner.lock() {
                Ok(mut slot) => slot.take().expect("source child ownership"),
                Err(poisoned) => poisoned
                    .into_inner()
                    .take()
                    .expect("source child ownership"),
            };
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => return Ok(status),
                    Ok(None) => {}
                    Err(error) => {
                        return Err(retain_for_background_reap(
                            child,
                            &format!("could not inspect source child: {error}"),
                        ));
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
            }
            .ok_or_else(|| {
                io::Error::other("source supervisor failed; child ownership unverified")
            })?;
            return match stop_and_reap(child) {
                Ok(_) => Err(io::Error::other(format!(
                    "could not start source supervisor: {error}"
                ))),
                Err(cleanup) => Err(io::Error::other(format!(
                    "could not start source supervisor: {error}; cleanup unverified: {cleanup}"
                ))),
            };
        }
    };
    Ok(SourceChild {
        stdout,
        stderr,
        stop,
        finished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reaper_stops_immediately_on_wait_error() {
        let calls = std::cell::Cell::new(0);
        let started = Instant::now();
        let error = bounded_reap_with(
            || {
                calls.set(calls.get() + 1);
                Err(io::Error::other("injected ECHILD"))
            },
            Duration::from_secs(3),
        )
        .unwrap_err();
        assert_eq!(calls.get(), 1);
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(error.to_string().contains("stopping retries"));
    }

    #[test]
    fn bounded_reaper_retries_interrupted_only_until_deadline() {
        let calls = std::cell::Cell::new(0);
        let started = Instant::now();
        let error = bounded_reap_with(
            || {
                calls.set(calls.get() + 1);
                Err(io::Error::new(io::ErrorKind::Interrupted, "injected EINTR"))
            },
            Duration::from_millis(60),
        )
        .unwrap_err();
        assert!(calls.get() >= 2);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn bounded_reaper_times_out_without_waiting_forever() {
        let started = Instant::now();
        let error = bounded_reap_with(|| Ok(None), Duration::from_millis(60)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn failed_reaper_spawn_returns_without_blocking_and_drops_owner() {
        struct DropMarker(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = Instant::now();
        let error = failed_reaper_spawn(
            Some(DropMarker(dropped.clone())),
            "injected supervisor failure",
            &io::Error::other("injected thread spawn failure"),
        );
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(error.to_string().contains("cleanup is unverified"));
        assert!(error.to_string().contains("no synchronous wait"));
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn stop_reaps_a_real_ffmpeg_source_when_configured() {
        let Some(ffmpeg) = std::env::var_os("TAU_TEST_FFMPEG") else {
            return;
        };
        let mut command = Command::new(ffmpeg);
        command.args([
            "-nostdin", "-v", "error", "-f", "lavfi", "-i", "anullsrc", "-f", "null", "-",
        ]);
        let child = spawn_source(&mut command).unwrap();
        child.stop.send(()).unwrap();
        let status = child.finished.join().unwrap().unwrap();
        assert!(!status.success());
    }
}

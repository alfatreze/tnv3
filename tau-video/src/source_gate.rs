//! Unintegrated Rust+C recovery draft. Not compiled or release-qualified.
use std::io::{self, BufRead, Read};
use std::sync::{mpsc::Sender, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

const LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Waiting,
    Pending,
    Accepted,
    Stopped,
}

struct State {
    decision: Decision,
    diagnostics: Vec<u8>,
    truncated: bool,
}

/// One gate per job; send only the first source-error prompt.
pub struct SourceGate {
    state: Mutex<State>,
    changed: Condvar,
    events: Sender<String>,
}

fn unavailable() -> io::Error {
    io::Error::other("source decision state unavailable")
}

fn stopped() -> io::Error {
    // Read::read_exact and BufRead helpers retry Interrupted, so cancellation
    // must use a non-retryable error kind.
    io::Error::other("encoding stopped")
}

impl SourceGate {
    pub fn new(always_continue: bool, events: Sender<String>) -> Self {
        Self {
            state: Mutex::new(State {
                decision: if always_continue { Decision::Accepted } else { Decision::Waiting },
                diagnostics: Vec::new(),
                truncated: false,
            }),
            changed: Condvar::new(),
            events,
        }
    }

    /// Record a bounded excerpt and return promptly so stderr continues draining.
    pub fn report(&self, bytes: &[u8]) -> io::Result<()> {
        let event = {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            let take = bytes.len().min(LIMIT.saturating_sub(state.diagnostics.len()));
            state.diagnostics.extend_from_slice(&bytes[..take]);
            state.truncated |= take < bytes.len();
            if state.decision == Decision::Stopped {
                return Err(stopped());
            }
            if !bytes.is_empty() && state.decision == Decision::Waiting {
                state.decision = Decision::Pending;
                Some(String::from_utf8_lossy(&state.diagnostics).into_owned())
            } else {
                None
            }
        };
        if let Some(event) = event {
            if self.events.send(event).is_err() {
                self.stop()?;
                return Err(io::Error::other("source decision receiver unavailable"));
            }
        }
        Ok(())
    }

    pub fn checkpoint(&self) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        while state.decision == Decision::Pending {
            state = self.changed.wait(state).map_err(|_| unavailable())?;
        }
        if state.decision == Decision::Stopped { Err(stopped()) } else { Ok(()) }
    }

    pub fn decide(&self, proceed: bool) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.decision != Decision::Pending {
            return Err(io::Error::other("no source decision pending"));
        }
        state.decision = if proceed { Decision::Accepted } else { Decision::Stopped };
        self.changed.notify_all();
        Ok(())
    }

    pub fn stop(&self) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        state.decision = Decision::Stopped;
        self.changed.notify_all();
        Ok(())
    }

    pub fn diagnostics(&self) -> io::Result<(String, bool)> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        Ok((String::from_utf8_lossy(&state.diagnostics).into_owned(), state.truncated))
    }

    /// Drain the current phase's diagnostics while preserving the job decision.
    pub fn take_diagnostics(&self) -> io::Result<(String, bool)> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let details = String::from_utf8_lossy(&state.diagnostics).into_owned();
        let truncated = state.truncated;
        state.diagnostics.clear();
        state.truncated = false;
        Ok((details, truncated))
    }
}

/// Drain a child diagnostic stream independently of the gated video reader.
/// The caller owns and must reap the child before joining this thread; this
/// helper does not stop the child or make inherited pipes close.
pub fn drain_diagnostics<R: Read + Send + 'static>(
    mut input: R,
    gate: Arc<SourceGate>,
) -> JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let result = (|| {
            let mut buffer = [0u8; 2048];
            let mut failure = None;
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    return match failure { Some(error) => Err(error), None => Ok(()) };
                }
                if failure.is_none() {
                    if let Err(error) = gate.report(&buffer[..count]) {
                        let _ = gate.stop();
                        failure = Some(error);
                    }
                }
            }
        })();
        if result.is_err() {
            let _ = gate.stop();
        }
        result
    })
}

/// Checks the gate before and after input reads; a blocked OS read cannot be
/// interrupted by this wrapper alone. Do not publish until upstream status and
/// job cancellation have also been checked.
pub struct GatedReader<R> {
    pub inner: R,
    pub gate: Arc<SourceGate>,
}

impl<R: Read> Read for GatedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.gate.checkpoint()?;
        let count = self.inner.read(buffer)?;
        self.gate.checkpoint()?;
        Ok(count)
    }
}

impl<R: BufRead> BufRead for GatedReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.gate.checkpoint()?;
        let data = self.inner.fill_buf()?;
        self.gate.checkpoint()?;
        Ok(data)
    }
    fn consume(&mut self, count: usize) {
        self.inner.consume(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn warning_waits_for_continue_and_only_prompts_once() {
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(false, tx));
        gate.report(b"source warning").unwrap();
        assert_eq!(rx.recv().unwrap(), "source warning");
        let worker_gate = gate.clone();
        let worker = thread::spawn(move || worker_gate.checkpoint());
        gate.decide(true).unwrap();
        assert!(worker.join().unwrap().is_ok());
        gate.report(b" more").unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn saved_choice_keeps_bounded_diagnostics_without_prompt() {
        let (tx, rx) = mpsc::channel();
        let gate = SourceGate::new(true, tx);
        gate.report(&vec![b'x'; LIMIT + 1]).unwrap();
        let (details, truncated) = gate.diagnostics().unwrap();
        assert_eq!(details.len(), LIMIT);
        assert!(truncated);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn taking_phase_diagnostics_preserves_accepted_decision() {
        let (tx, rx) = mpsc::channel();
        let gate = SourceGate::new(false, tx);
        gate.report(b"video").unwrap();
        rx.recv().unwrap();
        gate.decide(true).unwrap();
        assert_eq!(gate.take_diagnostics().unwrap().0, "video");
        gate.report(b"audio").unwrap();
        assert_eq!(gate.take_diagnostics().unwrap().0, "audio");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn disconnected_ui_fails_closed() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let gate = SourceGate::new(false, tx);
        assert!(gate.report(b"warning").is_err());
        assert_eq!(gate.checkpoint().unwrap_err().kind(), io::ErrorKind::Other);
    }

    #[test]
    fn cancellation_is_not_retried_by_read_exact() {
        let (tx, _rx) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(true, tx));
        gate.stop().unwrap();
        let mut input = GatedReader { inner: io::Cursor::new(b"frame"), gate };
        assert!(input.read_exact(&mut [0; 5]).is_err());
    }

    #[test]
    fn diagnostics_drain_does_not_wait_for_decision() {
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(false, tx));
        let reader = drain_diagnostics(io::Cursor::new(vec![b'e'; LIMIT * 4]), gate.clone());
        assert!(!rx.recv().unwrap().is_empty());
        assert!(reader.join().unwrap().is_ok());
        assert!(gate.diagnostics().unwrap().1);
        gate.decide(true).unwrap();
    }

    #[test]
    fn stop_wakes_a_pending_decision() {
        let (tx, rx) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(false, tx));
        gate.report(b"problem").unwrap();
        rx.recv().unwrap();
        let worker_gate = gate.clone();
        let worker = thread::spawn(move || worker_gate.checkpoint());
        gate.stop().unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(gate.decide(true).is_err());
    }

    #[test]
    fn diagnostic_read_failure_stops_gate() {
        struct Failure;
        impl Read for Failure {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("failed diagnostic pipe"))
            }
        }
        let (tx, _rx) = mpsc::channel();
        let gate = Arc::new(SourceGate::new(false, tx));
        assert!(drain_diagnostics(Failure, gate.clone()).join().unwrap().is_err());
        assert!(gate.checkpoint().is_err());
    }
}

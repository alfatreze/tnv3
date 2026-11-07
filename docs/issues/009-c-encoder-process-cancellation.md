# 009 — C encoder process-tree cancellation prototype

## 2026-11-05 update: native Rust+C direct-child control

**Host-tested on macOS; not confirmed on Pocket, Windows or Linux.** The
desktop native Rust+C path now supervises its source process, direct C segment
workers and mux/check helpers, drains diagnostics independently, and serializes
Stop against publication. It retains the workspace when worker cleanup cannot
be verified. The app shows the custom-tool restriction. This update does not
supersede the rejected process-group approach below: arbitrary descendants,
external reapers and supervisor crashes are not contained, and Python still
stops at a phase boundary. Real FFmpeg/C tests and the source-error Stop test
passed on the macOS host; custom wrappers and hardware playback remain gates.

**Status:** isolated source prototype; integration rejected/deferred and not
release-qualified. Host-tested on macOS arm64 only. The current process-group
approach cannot guarantee descendant teardown or eliminate the PGID reuse race
when an external SIGCHLD reaper is active. Do not wire it into the app or CLI.
Review follow-up addressed busy-progress polling, ESRCH/ECHILD handling,
bounded pipe-reader completion, and loss-safe progress-reader error reporting.

## Observed and expected behavior

The existing `c_encoder::encode_video` waits synchronously while reading
progress lines from the Python orchestrator. It has no cancellation input, and
terminating only the Rust `Child` may leave Python's FFmpeg/FFprobe and parallel
C encoder descendants running. The prototype can interrupt quiet encodes and
signal a process group best-effort, but cannot reliably prove that all
descendants stopped or that the group identifier still belongs to the job.

## Prototype decision

Preserve the existing synchronous API. Add Linux/macOS-only
`encode_video_with_cancellation` with a cloneable atomic cancellation token.
Launch Python in a new process group, drain progress on a reader thread, and
poll progress/cancellation on the caller thread. SIGTERM is sent only while the
leader is waitable; SIGKILL escalation is attempted only if it remains waitable
after 400 ms. If it exits or is externally reaped, the adapter refuses to
signal by a possibly reused process-group ID and reports descendant cleanup as
unverified. An external SIGCHLD handler or auto-reap policy can invalidate the
leader PID/PGID between observation and signaling. ECHILD detects some cases,
but cannot close the race. There is no portable race-free API here to both pin
a process-group ID and prove all members have exited. Accordingly this
prototype is rejected for application integration. The existing synchronous API uses
its historical direct-child behavior, preserves each progress callback, and
drains stdout through EOF. Windows cancellation is not exposed; it needs Job
Objects with kill-on-close and native teardown/reap tests. `rustix` is now an
optional dependency activated only by the `c-encoder` feature.

## Evidence and limits

- **Host:** `cargo test --offline --features c-encoder` passed 25 library
  tests, 1 adapter test, 1 intra reference test, 5 mux tests, 5 reader tests,
  and 1 video reference test.
- **Host:** a fake shell orchestrator closed stdout, ignored SIGTERM, and
  launched a delayed descendant. Cancellation returned with an untouched
  writer and the descendant did not create its sentinel, exercising polling,
  TERM-to-KILL escalation, and group teardown.
- **Host:** a fake orchestrator exited while a delayed descendant remained;
  the descendant wrote its sentinel. This verifies and preserves the stated
  limitation: the adapter does not signal a process group after leader exit.
- **Host:** a fake orchestrator emitted a sustained stdout stream and exited
  while a delayed descendant retained the pipe; the leader was detected
  despite the nonempty progress queue, and descendant cleanup was reported as
  unverified rather than signaling a potentially stale group ID.
- **Host:** targeted cancellation tests passed 5/5; full crate suite passed
  25 library tests, 1 adapter test, 1 intra reference test, 5 mux tests,
  5 reader tests, and 1 video reference test; `cargo fmt --check` passed.
- **Code-review:** `cargo check --offline --tests --target
  x86_64-pc-windows-msvc` passed for the unchanged Windows synchronous path.
  This is a cross-target compile only, not Windows runtime evidence.

The tests use fake shell processes, not a live FFmpeg/C encode, and ran only on
macOS. Linux runtime behavior, real multi-segment C worker cancellation,
external-reaper behavior, nonzero/timeout teardown, and native Windows Job
Object handling remain open. The cancellation API cfg is Linux/macOS only; it
is not a generic Unix implementation.
Process-group signaling is best effort while the leader remains waitable. A
descendant can outlive the leader; the prototype intentionally does not signal
the numeric PGID after that point and reports cleanup as unverified. A helper
that deliberately creates a new session/process group can also escape this API.
Pipe readers are given 500 ms to finish after process-group cleanup. If an
escaped process retains a pipe, the call returns without an indefinite join;
its detached reader thread can remain until that pipe closes. Progress is
advisory and may be coalesced under queue backpressure; read errors are tracked
out-of-band so queue saturation cannot suppress them.
Cancellation during the final caller-writer copy can leave partial bytes; the
caller should stage output and discard it on error. The desktop UI and CLI have
not been switched to the new API.

## Safe workaround and next gate

Keep the synchronous API until a stronger cancellation architecture is
designed. The next gate is to select and validate a supervisor mechanism that
tracks children independently of a reusable PGID, or establish and enforce a
process contract that gives the adapter reliable ownership of every child;
then test teardown against external-reaper behavior on Linux and macOS. Do not
wire cancellation into the UI/CLI or claim process-tree absence before that
gate. Windows remains unsupported pending a separate Job Object design and
native tests.

# 012: Source error decision and preflight

## Decision and scope — 2026-11-05

The initial error-decision implementation runs a separate FFmpeg source check
before encoding unless the user has saved the always-continue preference. A
recognized Matroska or HEVC error stops this direct child check, collects a
bounded diagnostic and asks whether to continue; other diagnostics are offered
after successful completion of the check. A failed check fails the job.
Stopping the check or declining the warning occurs before any output pair is
created. The saved preference bypasses the check; existing encoder diagnostics
still appear after encoding. FFmpeg errors may be fatal or recoverable, and
continuation does not establish source repair.

A separate preflight avoids invoking the existing unverified Python/C
process-tree cancellation prototype, but can cost a full extra decode. It does
not ask about new errors that appear only during the subsequent encode. The
Stop control is for the source check only; there is no stop control for an
active encode. An external wrapper that keeps the diagnostic pipe open can
leave cleanup unverified, which is reported as an error. See issue 009 for the
remaining process supervisor and Windows Job Object work.

**Host:** controlled FFmpeg substitutes exercised clean, warning, failure and
stop cases, including a closed diagnostic pipe; the previously reported
corrupt source produced a warning. A synthetic browser preview exercised the
dialog and preference persistence. These do not establish long-film, native
Windows/Linux, or Pocket behavior. Next gate: design safe live error decisions
and cancellation across the actual encoder and all child processes, then test
real multi-segment jobs, cleanup and error cases on each supported platform.

**Cross-compile:** Windows app `cargo check` passed from macOS after adding the
installed Homebrew LLVM tools to PATH. An initial attempt failed because
`llvm-rc` was not on PATH; this does not establish Windows runtime behavior.

## Phase-boundary decision — 2026-11-05

Version 0.7.0 also handles source diagnostics that appear after a clean
preflight: after a successful video or audio phase, and before final output
publication, it requests a continue/stop decision. An accepted source warning
is remembered for the current job; the saved preference suppresses the
dialog. A user stop returns before publishing the TNV3/MP3 pair. This is a
phase-boundary decision, not a live pause or immediate cancellation: active
FFmpeg, Python and C workers still run to the phase boundary. Fatal subprocess
failures still fail the job. If the app window disappears while a decision is
pending, the job stops before output publication.

**Host:** generated inputs and a diagnostic-emitting FFmpeg wrapper exercised
stop and continue in both C workflows; no final files were created after stop.
Native Windows/Linux and Pocket behavior remain untested. The supervisor work
in issue 009 remains the gate for immediate cancellation and true live prompts.

## Cooperative encode stop — 2026-11-05

Version 0.8.0 permits a stop request during video or audio processing. It
waits for the current phase and its child processes to finish naturally, then
discards staged output and exits before publishing either file. Stop and the
start of publication are serialized: once saving starts, a stop request is
refused. This can take the duration of an entire video phase; it is not
immediate process-tree termination and does not resolve issue 009.

**Host:** generated FFmpeg/C tests requested stop while processing both C
workflows, confirmed that neither final file was published, and verified
stop/publication ordering under concurrent requests. Native Windows/Linux
runtime and Pocket remain untested.

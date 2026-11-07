# 015: CLI cleanup fallback can wait indefinitely

Date: 2026-11-06. Status: open. Evidence: **code-review**, no fault-injected host reproduction yet.

The native CLI's direct-child supervisor retries `Child::wait` indefinitely after any error in a detached reaper. A permanent error such as an externally reaped child can cause an endless retry. If starting the reaper thread fails, its fallback waits synchronously and can block the CLI indefinitely. This is an exceptional cleanup path; it does not establish that ordinary encodes hang.

Expected: a verified reap, or a bounded return explicitly reporting that cleanup is unverified; no numeric-PID signal after ownership is lost. Impact: an exceptional cleanup failure can hang the CLI or leak a reaper thread. Workaround: use the existing Python workflow or restart the CLI if an exceptional native cleanup hangs; do not remove named retained staging until the tool has stopped. Next: test and independently review a narrow direct-child cleanup fix, preserving the existing no-publication behavior when cleanup cannot be verified. Process-group and whole-tree cancellation remain separate gates.

## 2026-11-06 bounded fallback integrated

**Host / code-review:** The CLI fallback now polls reaping for at most three seconds, retries interrupted waits only until that deadline, and stops on permanent errors. If its reaper thread cannot start, it returns an explicit `cleanup is unverified` error without synchronously waiting. Injected wait-error, timeout, interruption and thread-start-failure tests passed; real-tool stdin-cancellation tests also passed on macOS. This resolves the unbounded retry/synchronous-wait paths identified above. It does not guarantee eventual reaping: a child can remain alive or unreaped after the bound, so staging is retained and process-tree handling remains open.

## Related Rust+C worker path

**Code-review:** The Rust+C orchestrator also has an unbounded reaper retry and synchronous fallback in `tau-video/src/experimental_c_orchestrator.rs`; its termination path can additionally block in `wait()` after a failed kill. The V-350 CLI supervisor fix does not cover this separate worker path. A narrow bounded fix is being staged and must retain the existing `cleanup is unverified` signal and workspace-preservation behavior.

## 2026-11-06 Rust+C worker fallback

**Host / code-review:** The separate Rust+C worker path now uses bounded polling after termination attempts and in its exceptional reaper. Permanent errors stop retries, interrupted waits are bounded, and a failed reaper-thread start returns an explicit `cleanup is unverified` result without blocking the caller. The native workspace remains retained if cleanup cannot be verified. Four injected helper tests, the 14-test orchestrator group, the 53-unit-test feature suite and three real-tool stdin-cancellation tests passed on macOS. This does not guarantee eventual reaping, descendant cleanup or a whole-operation cancellation deadline; blocked worker I/O can still delay joins.

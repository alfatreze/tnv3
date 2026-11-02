# Cross-platform CLI migration plan: process-backed workflow to native C API

**Status:** proposed plan; no implementation or platform validation is claimed.  
**Date:** 2026-11-01  
**Scope:** incrementally migrate the current Python/C process workflow to a cross-platform CLI and eventually a C library with a narrow Rust FFI. Preserve the desktop TNV3 + MP3 output pair and make the Rust crate core process-free as a migration goal.

## Starting point and decision

The current default `tau-video::c_encoder` path is a safe Rust **process adapter** around Python and C tools. The app uses it for C encoding; FFmpeg/FFprobe and MP3 sidecar generation are also external processes. The experimental pure Rust TVC encoder is feature-gated and is not compression-equivalent to C. The Rust crate's format/core APIs must remain free of process execution, but the current default adapter is process-backed. The migration should first make that workflow explicit and reliable as a CLI boundary, then reduce Python orchestration, and only later consider extracting the mature C implementation as a library and binding it through FFI. This interim process-backed CLI/workflow does **not** satisfy Tau Omega's pure-Rust crate milestones in `docs/RUST_ROADMAP.md`: it is an integration/workflow bridge, not the pure-Rust checker, decoder, segmented encoder, deterministic resume, or TAC/audio implementation. The dated Omega requirement set calls for in-container TAC and a pure-Rust/resumable encoder; the later parity/app decision parks the experimental Rust encoder and uses a temporary MP3 sidecar. Treat that as an explicit staged divergence and decision requiring reconciliation with the Omega owner/spec, not as proof the Omega contract is complete or silently superseded. Track the Omega milestones and their evidence separately; CLI progress cannot be reported as completion of an Omega crate milestone.

Keep the C encoder as the reference while evidence is gathered. The parity review's same-QP sample is not an equal-quality comparison and does not establish Pocket behavior. Do not promote the experimental Rust encoder based on this migration. A Pocket playback fixture is a **release gate** for changes that affect the shipped Pocket pair/player path; its absence must not block host-only CLI/library planning and development. Do not state or imply that a known-good archived Pocket pair currently exists until one is identified and verified.

## Product and platform invariants

1. Preserve the app's matched `.tnv3` + `.mp3` outputs, `_C` / `_R` naming choices where exposed, and current no-overwrite behavior. Describe pair publication as best-effort coordinated publication, never as atomic across two files.
2. Preserve current encoder defaults and the C/Pocket player pairing while adding migration paths. Keep an explicit rollback to the existing process workflow until native-platform and output gates pass.
3. Keep `tau-video` format parsing, validation, decoding, and reports safe and process-free. Process execution remains in explicit CLI/app adapters. FFI remains isolated behind an optional adapter crate/feature; the no-FFI core must build without a C toolchain.
4. Treat macOS, Windows, and Linux as separate native targets. Pass paths as structured/native path values with documented conversion; do not require shell quoting or a platform shell. Native path types at the Rust/app layer do **not** make existing Windows C tools Unicode-safe: audit every narrow `argv`/`fopen` boundary. Add wide-argument entrypoints and `_wfopen`-family handling, or prove a deliberate ASCII-safe staging conversion that works with arbitrary Unicode input/output/temp roots and preserves no-overwrite semantics.
5. A verified Pocket fixture is required before release claims for Pocket compatibility. Host validation can proceed independently and must be labeled host evidence.

## Output profiles

Make output intent explicit instead of burying it in defaults or scattered arguments. A profile is a named, versioned configuration that resolves to a fully reported effective configuration before work starts.

- **Pocket compatibility:** current supported Pocket dimensions/frame rate, current selected C encoder defaults, video-only TNV3 plus matching 48 kHz stereo 128 kbit/s MP3 sidecar as applicable. This profile preserves existing behavior and is the candidate for hardware validation; it must not be called hardware-confirmed until played on Pocket.
- **Host preview:** fast local iteration and desktop preview, with explicit dimensions, frame rate, quality/rate settings, and sidecar policy. Never imply that desktop timing predicts Pocket performance.
- **Custom:** caller-specified settings with validation against format/level bounds and a saved effective-config report.

Each profile needs a stable profile ID/version, resolved options in the job report, and regression coverage. Profile changes that alter format, rate, dimensions, timestamps, or audio policy require an explicit compatibility review. App UI can still expose overrides, but the final effective settings must be inspectable and reproducible.

## Staged architecture

```text
Desktop UI / tau-video CLI
  ├─ workflow adapter (staging, reports, cancellation, pair publication)
  ├─ source/audio adapters (FFmpeg/FFprobe processes initially)
  ├─ C backend: current process adapter -> later optional C-library FFI
  ├─ experimental Rust backend (feature-gated, not at C parity)
  └─ safe tau-video core (format IO, validation, decode; no processes)
```

## Phases and gates

### Phase 0 — inventory and baseline (host work may proceed without Pocket evidence)

- Inventory `tau-video`, `tau-encoder`, `tools/tnv3_ref/encode_film.py`, C tools, make targets, process arguments, output names, diagnostics, progress, cancellation limits, tool discovery, and packaging. Record existing exit codes and report semantics.
- Define the output profiles above and write down how each resolves to current settings. Keep current effective defaults as the compatibility profile until changed through a separate decision.
- Build a host corpus across aligned/unaligned dimensions, segment boundaries, rate-window edges, audio variants, Unicode/spaced paths, malformed inputs, and existing-file destinations. Keep licensed/private media local as appropriate.
- Identify whether a Pocket fixture already exists. If none is available, record the release gate as pending rather than blocking host work or inventing fixture evidence.
- **Gate:** contracts, profile definitions, and reproducible host inputs exist before changing workflow behavior. Add a recorded reconciliation decision for the dated Omega in-container TAC/pure-Rust/resume requirements versus the later temporary MP3/parked-Rust path; until resolved, report those Omega requirements as outstanding/divergent.

### Phase 1 — process-backed CLI/workflow first

- Establish one explicit CLI workflow that invokes the current Python/C pipeline using structured arguments/config, with documented backend selection, tool paths, output profile, progress/report format, and exit/error mapping. Reuse existing tools; do not extract C yet.
- Make the workflow reproducible and cross-platform in path handling, temporary directories, quoting avoidance, diagnostics, and cleanup. Keep process invocation in the CLI/app edge; the format library remains uninvolved. On Windows, explicitly bridge wide filesystem paths to all C executables: narrow `argv` and `fopen` in current tools do not guarantee Unicode behavior. Prefer wide-argv plus `_wfopen`-compatible handling; if using ASCII-safe staged aliases instead, test conversion, collisions, cleanup, permissions, and temp roots on non-ASCII paths.
- Ensure every job produces a machine-readable report containing profile ID/version, effective options, tool versions/paths (as appropriate), inputs/outputs, frame/segment counts, stage timings, and failure stage. Avoid leaking arbitrary environment state into reports.
- Implement cancellation for the **whole process tree**, not only the immediate child. On POSIX, launch each job in its own process group/session and terminate then reap the group with a bounded graceful-to-forced escalation. On Windows, place the job in a Job Object configured to terminate descendants and wait for teardown. Cover Python -> FFmpeg/FFprobe and Python -> concurrent `tvc_enc` segment workers, including grandchildren, early parent exit, timeout, and user cancellation. Drain/close pipes, reap all children, and only then clean temp files. Document platform-specific limitations if tree-wide termination cannot be guaranteed.
- **Gate:** legacy workflow and CLI produce equivalent output metadata and independently decoded video on the host corpus. Exercise paths with spaces/non-ASCII on every OS, including Windows Unicode source, output, script/tool paths, and a non-ASCII temp root; verify the actual C file-open path rather than only Rust path construction. Also exercise missing tools, nonzero process exits, truncated output, cancellation while FFmpeg runs and while multiple `tvc_enc` children run, orphan-process detection, and no-overwrite. Pocket evidence is not required for this host-only gate. Passing this gate still does not satisfy Omega's pure-Rust crate milestones.

### Phase 2 — robust pair staging and publication

- Stage each output to a unique temporary file on the destination filesystem **before** encoding starts; no final destination is visible yet. Validate the staged outputs before publishing. For TNV3, `tools/tnv3_ref/tnv3 check` is the current reference checker, but it belongs to the same C reference toolchain and is **not an independent implementation**. Use it as a baseline cross-check only; record binary/source revision and its documented exit/result. Python demux and Rust full decode are candidate independent paths only after their format coverage, validation behavior, and malformed-input gaps are audited and demonstrated for the relevant files. Compare frame count/order, dimensions, timestamps, index/CRC semantics, and decoded pixels where possible. `tau-video` currently checks header/declared layout and reads index entries; it explicitly is **not** a complete TNV3 checker. Full level/stream validation and malformed-input coverage remain gaps. Until an implementation with demonstrated independent coverage exists, label validation as reference-tool checking plus decode/structural checks, not independent full-container validation. For MP3, use FFprobe to inspect stream presence, codec, sample rate, channels, and duration against profile tolerance; this checks metadata, not audible correctness or A/V synchronization. Record those remaining gaps, and add an independent decode/PCM-duration check where practical.
- Before publication, confirm neither final destination exists. Publish each file with a per-file **no-replace** operation (platform-appropriate exclusive rename/link semantics). Never overwrite a destination.
- Two files cannot be atomically published as one unit with ordinary filesystem rename. There is an interval where observers may see only one final file; a crash between the two publishes may leave a one-file pair. If the second publish fails, attempt best-effort removal of the first published file, but rollback can also fail (permissions, filesystem fault, crash). Preserve staged/recovery artifacts or emit a recovery manifest when safe cleanup is uncertain. Report exactly which files were published and which cleanup/rollback actions failed.
- On startup or next run, detect stale staged artifacts/recovery manifests and offer deterministic cleanup/recovery; do not silently pair an unrelated MP3 with a TNV3. A small manifest/commit marker can help the app identify a completed pair, but cannot make two filesystem names atomically visible to external observers.
- **Gate:** fault-injection tests at every stage (before validation, after validation, after first publish, second publish failure, rollback failure) demonstrate no overwrite and explainable recovery. Verify filesystem/platform semantics on each supported OS.

### Phase 3 — reduce Python orchestration

- Move orchestration responsibilities one by one into a typed CLI/workflow layer: profile resolution, source metadata, segment plan, progress aggregation, report generation, validation, and cleanup. Keep the mature C encoder and its existing standalone CLI as the codec backend.
- During each extraction, run old and new orchestration against the same input and compare independent decode results, container metadata, and reports. Retain the Python workflow as rollback until all supported options and error paths are covered.
- Decide explicitly whether FFmpeg/FFprobe and MP3 remain processes. They may remain process-backed at app/CLI boundaries; the Rust core must not spawn them. Do not claim the app has no external dependencies while they remain.
- **Gate:** the Python layer is no longer required for the supported normal CLI/app path only after parity, diagnostics, process-tree cancellation, and packaging gates pass. Keep Python developer/reference tools where useful. This does not by itself meet Omega's pure-Rust milestone requirements while native C tools or subprocess-backed media adapters remain.

### Phase 4 — evaluate C library extraction (later, conditional)

- First determine whether process-backed CLI performance, packaging, or integration has a concrete blocker that warrants C ABI maintenance. If not, retain the process-backed executable as the stable boundary; FFI is optional work, not a presumed endpoint.
- If justified, refactor the C executable into a thin CLI over a library without changing codec decisions. Introduce opaque per-encode context; move mutable encoder, scratch, segment, error, progress, cancellation, and temp-file state into it. Audit globals, static mutable storage, random state, signal handlers, and shared buffers. Immutable tables may remain shared and `const`.
- Define stable statuses (success, invalid config/input, unsupported, IO, allocation, codec/internal failure, cancelled) plus structured context-owned diagnostics: stable code/message, stage, frame/segment, and native OS code where available. No transient string pointers, `exit`/`abort`, shell calls, global mutable error state, cwd/environment mutation, locale mutation, or library-owned signal handlers.
- Specify ABI version/struct-size negotiation, allocator ownership, path encoding, callback thread/reentrancy/lifetime, and context lifecycle. Use fixed-width types and opaque handles; avoid compiler-dependent public structs, `FILE*`, C++ types, and Rust types. Prevent exceptions/unwinding across the ABI. Initially serialize calls if reentrancy cannot be proven; only enable concurrent contexts after tests and memory measurements.
- Keep library build and FFI optional. Put unsafe declarations in a small `tau-c-encoder-sys` crate and safe RAII/error conversion in a wrapper. `tau-video` core and `--no-default-features` builds must not require the C toolchain. FFmpeg and MP3 process adapters remain outside the core.
- **Gate:** old CLI vs library output passes independent decode and metadata checks; C lifecycle/error/callback/cancellation/concurrent-context tests pass under available sanitizers; Rust wrapper tests cover null/length/status/panic/Drop/cancellation boundaries. No algorithm change is bundled into extraction.

### Phase 5 — native platform qualification and staged rollout

- Build/test natively on macOS ARM64, Windows x64, and Linux x64: CLI, app, optional native library, FFI and no-FFI Rust core. Verify clean installation without developer-only toolchains, DLL/shared-library loading, runtime compatibility, signing/notarization requirements, and non-ASCII/spaced paths. Native Windows tests must confirm Unicode paths through the complete process and C-tool chain; Rust-native path APIs alone are insufficient evidence. Gate release on wide-argv/`_wfopen` support or on demonstrated ASCII-safe staging for non-ASCII source, destination, executable/script, and temp-root paths. Until each dependency is removed or replaced, runtime packaging/discovery must cover Python 3, `encode_film.py`, `tvc_enc`, `tnv3`, FFmpeg, FFprobe, and FFmpeg `libmp3lame` support (plus platform runtime libraries). Removing Python orchestration alone does not remove the C tools or FFmpeg dependencies; update this inventory only after a dependency is eliminated and its replacement is qualified.
- Compare process and any library backends over the host corpus with independent decode, format checks, quality/rate/time/memory metrics. Label all evidence by host platform; a successful simulated or CI build is not Pocket evidence.
- Keep the existing process backend as fallback through a canary/release period. Change defaults only after all supported OS gates and rollback have been demonstrated.
- **Pocket release gate:** identify or create a reproducible fixture, record source and tool/player revisions, options and TNV3/MP3 hashes, then test playback, frame/audio counters, sync, and failure behavior on actual Pocket hardware. If no fixture/device evidence is available, leave the Pocket release claim pending; host development and host release work can still proceed within scope.

## State, errors, and test contract

- Workflow states are explicit: resolve profile -> preflight tools -> create a per-job staging area and unique staged TNV3/MP3 paths -> ingest -> encode video -> encode audio -> validate both staged outputs -> publish video -> publish audio -> complete. Each error names the stage and preserves diagnostic context.
- Per-job mutable state is isolated. If C FFI is adopted, each call owns an opaque context with deterministic lifecycle; invalid/repeated transitions fail consistently.
- Cancellation semantics are stage-specific and documented. Process-tree termination/reaping (including FFmpeg and concurrent `tvc_enc` descendants), C hot-loop cancellation if a library is later adopted, cleanup, and publication rollback are distinct outcomes.
- Reports distinguish successful encoding from successful pair publication. Report partial publication and rollback/recovery failure without euphemism.
- Validation uses independent implementations where available; state exact tool/version and scope. If only structural checks are performed, say so. A successful encoder exit code is not independent file validation.

## Validation matrix

| Layer | Required evidence |
| --- | --- |
| Process CLI/workflow | explicit config/profile, exit/error map, report schema, spaces/non-ASCII paths on every OS; on Windows test wide source/output/tool paths and non-ASCII temp roots through C `argv`/file opens or verified ASCII-safe staging; missing tools, child failure, cancellation and cleanup |
| Pair publisher | TNV3 reference checker clearly labeled same-toolchain plus candidate independent decode only after coverage audit; MP3 stream/rate/channel/duration metadata checks; no-replace semantics; injected crash/failure at each publish step; recovery behavior |
| C library (only if adopted) | lifecycle, status/error ownership, allocation failure, callbacks, cancellation, serialized/concurrent contexts, sanitizer runs |
| Rust wrapper (only if adopted) | RAII/Drop, typed error mapping, path/length conversion, callback panic containment, optional feature build; verify core has no process API dependency |
| App | matching TNV3/MP3 pair, profile/effective settings, stage progress/errors, output folder persistence, no-overwrite and partial-pair recovery |
| Cross-platform | native macOS/Windows/Linux builds and runtime tests, clean packaging, helper discovery, no developer-only assumptions |
| Codec/container | reference checker result clearly labeled as same-toolchain; separate decoder comparisons; frame order/count, dimensions/timestamps/index/CRC checks, audio metadata/duration, quality/rate/time/memory corpus; no independent full-checker claim until coverage is established |
| Pocket release gate | actual Pocket hardware, player/build identity, pair hashes, frame/audio counters and sync; pending until evidence exists |

## Risks and decision points

- Pair publication cannot be atomic across two separate files. The design can prevent overwrites, detect incomplete pairs, and attempt rollback/recovery, but cannot guarantee observers never see a partial pair or that rollback survives a crash/filesystem failure.
- C reentrancy may be costly. If global state cannot be removed safely, a single serialized library context may be acceptable, but measure and document the throughput cost before adoption.
- Python removal, C library extraction, Rust FFI, and Rust encoder parity are separate decisions. Do not bundle them into one migration or use FFI as a presumed shortcut to Rust purity.
- FFmpeg/FFprobe and MP3 remain external process dependencies until a separate decision removes them. During process-backed phases, runtime packaging/discovery includes Python 3, `encode_film.py`, `tvc_enc`, `tnv3`, FFmpeg, FFprobe, and `libmp3lame` support as applicable; do not claim a self-contained or pure-Rust solution while any remain.
- Windows/Linux app runtime and native helper packaging remain unverified per current README. Current narrow Windows `argv`/`fopen` use in C tools is a specific Unicode-path risk; native Rust path types do not resolve it. This plan does not assert compatibility until wide-path handling or ASCII-safe staging passes native end-to-end tests.
- Changes affecting the Pocket output/player path require hardware evidence before release claims; this does not block host-only phases. The temporary MP3 sidecar and parked Rust encoder are later app/parity decisions that diverge from the dated Omega in-container TAC and pure-Rust/resume requirements; document the owner-approved reconciliation before marking the Omega contract fulfilled.

## Source records

- `docs/RUST_C_ENCODER_PARITY.md` — current process-adapter decision, parity evidence, and gates.
- `tau-video/README.md` — safe Rust crate, default process adapter, optional experimental encoder, CLI and dependencies.
- `tau-encoder/README.md` — desktop pair semantics, C/Python requirements, platform packaging and validation gaps.
- `README.md` — C reference workflow, Rust status, and platform evidence limits.
- `docs/` and `docs/issues/` — codec trade-offs, performance reports, and unresolved failures; consult/update the relevant records as implementation proceeds.

## Review follow-up: minimum host gate and recovery

For the first host-only Pocket profile gate, require the reference `tnv3 check`
to succeed, a complete decode by the C TVC decoder after demux, and explicit
frame-count, dimensions, timestamp/index and MP3 stream/duration checks.
Document the exact coverage and limitations; this is not an independent full
container validation or Pocket playback certification.

For crash recovery, specify where durable manifests are named and discovered,
which staged files they own, and the order for syncing staged contents,
manifest and parent directory where the platform/filesystem supports it.
Measure actual guarantees; run killed-process fault tests at each publication
step on target filesystems including FAT/exFAT, and test no-replace methods
without assuming hard links are available. Where guarantees are unavailable,
report the residual crash/visibility limitations and recovery procedure.

Consumer requirements: `Tau Omega/docs/TAU_VIDEO_CRATE_REQUIREMENTS.md` in
the sibling project; reconcile its dated milestones with subsequent owner
decisions rather than claiming the process-backed workflow fulfills them.

## Execution started

**Host, 2026-11-01:** `tau-video` now defaults to no features; callers of the
process adapter must explicitly enable `c-encoder`, which the desktop app
already does. Generated C helper smoke coverage is available in
`tools/codec_study/cli_migration_smoke.py`; it requires an explicit codec root
and configured/built reference tools. It covers one synthetic one-second
segment, not app integration, segmentation, full validator coverage or Pocket
hardware. The normal desktop C path was separately host-tested.

**Prototype progress:** `tau-video-cli` contains an experimental video-only
C-backed CLI, tested on generated media on macOS. It does not yet produce the
Pocket pair; Windows publication is unsupported and Linux/card filesystems
remain unverified. It is not a release gate completion. See issue 008.

**Further prototype progress:** the CLI now has an opt-in `pocket-sidecar`
profile, with generated macOS success/failure and no-overwrite smoke coverage.
It uses best-effort pair rollback and retains the limitations in issue 008.
This is not the complete crash-recovery or cross-platform publication gate.

**Cancellation investigation:** a Linux/macOS process-group prototype was
not integrated: descendant teardown and numeric PGID lifetime cannot be
guaranteed under external reaping. Establish a stronger supervisor or
enforceable process-ownership contract before wiring cancellation into the
CLI/app. Host-only evidence and limits are in issue 009.

## MVP priority, 2026-11-01

Owner direction: prioritize a usable macOS TNV3/MP3 workflow; provide basic
Windows and Linux builds with limited testing. MP3 is a temporary sidecar and
does not need exhaustive robustness work before the planned audio replacement.
Keep existing output protection, be explicit about incomplete pair atomicity
and platform/dependency limits, and defer broad hardening rather than claiming
the longer release gates have passed.

**Python migration groundwork:** `tau_video::y4m` streams bounded 8-bit 4:2:0
frame groups into caller-owned sinks. Host-generated parity and error tests
pass. Production encoding still uses Python; the caller must check the
producer exit status and manage incomplete sinks and concurrency.

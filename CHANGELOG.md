# Changelog

## Unreleased — CLI contract work

- Bounded Rust+C worker termination/reaping on exceptional cleanup, retaining the workspace when cleanup cannot be verified.

- Bound exceptional CLI child reaping and report unverified cleanup instead of waiting indefinitely.

- Probe can decode one frame with the default reference filter and report coded geometry and minimum geometry-fitting level.

- Added an explicit-path tool identity query with bounded SHA-256 hashing; versions remain unreported where unavailable.

- Added explicit `--allow-nonatomic-copy` compatibility mode for filesystems without atomic publication. This mode can leave partial files and remains subject to target-card testing.

- Added a combined container-preview check with VALID/INVALID/INDETERMINATE outcomes; it does not decode codec payloads or claim full conformance.

- On macOS/Linux, replace the potentially partial visible-file copy fallback with atomic hard-link fallback or a clear failure with retained staging. Some card filesystems may need a different output location.

- Added a macOS-host-tested experimental source metadata `probe`; size and time estimates remain unavailable.

- Added a bounded subtitle-structure check to the Rust reader and CLI; full conformance remains incomplete.

- Extended the partial cross-frame check to validate audio interleaving and rolling picture/chunk byte-rate limits.

- Added a fail-fast cross-frame scope to the CLI check, without claiming full conformance.

- Added a partial Rust cross-frame reader check with bounded audio bookkeeping; it does not establish full conformance.

- Added explicitly scoped CLI chunk-structure checking; the full conformance check remains unavailable.

- Added process-free bounded picture-chunk structure scanning; this is not a complete container checker.
- Added workflow settings to the CLI plan event, distinguishing requested QP and worker limits from measured output.
- Classify automatic missing dependencies as exit code 3 rather than usage code 2; JSON error events add a `kind`.

- Added preliminary JSON/human `info` for header and declared-layout metadata; index entries and chunks are not checked.
- Added adjacent executable discovery with explicit tool and checkout precedence.
- Added JSON error and metadata command tests. A `probe` draft is withheld pending safe process and pipe cleanup.

## Tau codecs CLI 0.3.0 — version, typed-error and cancellation preview

- Add `--version` and `--version --json`, reporting the CLI and TNV3 container versions without claiming external tool versions.
- Add typed C process-adapter errors while retaining the existing string APIs; CLI process and I/O failures use those types where available.
- Added opt-in native `--control-stdin` cancellation for trusted directly owned leaf tools; verified stops return code 4. Cleanup uncertainty is an error and may retain staging. Host-tested on macOS.
- Python and SIGINT cancellation, full process-tree cancellation, deterministic resume and complete error classification remain open.

## Tau codecs CLI 0.2.0 — machine-event development preview

- Add preliminary schema-1 JSON-lines plan, progress, warning, done and error events, with output sizes and SHA-256 hashes.
- Preserve the current C workflow defaults. Cancellation, deterministic resume and complete stable error classification remain unimplemented.
- Add bounded indexed TNV3 picture access to the Rust library; it is not a full container checker.

## Tau Encoder 0.9.0 — native recovery development preview

- Ask during Rust+C video and audio decoding when FFmpeg reports source errors; a saved preference can continue automatically.
- Stop supervised native decoding and direct C workers before publishing the output pair; report unverified cleanup and retain affected temporary files where necessary.
- Python retains its preflight and phase-boundary stop. Custom tools that spawn descendants are not fully contained; Windows and Linux runtime behaviour is untested.

## Tau Encoder 0.8.0 — cooperative stop development preview

- Allow stopping an encode after the current processing phase, before final files are published.
- Active workers finish the phase; immediate process-tree termination remains future work.

## Tau Encoder 0.7.0 — source decision development preview

- Ask about unacknowledged source errors after video or audio processing and before publishing output files.
- The running phase still completes before the prompt; immediate encoder cancellation remains future work.

## Tau Encoder 0.6.0 — source-check development preview

- Add a stoppable source preflight, source-error decision and saved continue preference.
- Active encode cancellation and live encoder-error decisions remain future work.

## Tau Encoder 0.5.0 — desktop preview

- Select output width, including 240 and 360 pixels, and determine height from the source.
- Show the actual scaled dimensions and compatible level; prevent selecting an incompatible manual level.

## Tau Encoder 0.4.0 — desktop preview

- Select output dimensions and TVC level in both C workflows.
- Preview at native size or at 2× and 4×.

## Tau Encoder 0.3.0 — desktop preview

- Show automatic and detected dependency paths in Settings and allow choosing an installation.
- Update the macOS FFmpeg helper bundle to 9.0.2.

## Tau Encoder 0.2.2 — desktop preview

- Explain common Matroska and HEVC source warnings and their possible effects, with the original diagnostics available on demand.
- Display the app version to make it easier to confirm which build is running.

## Tau Encoder 0.2.1 — desktop preview

- Report completed video segments and update the remaining-time estimate in the experimental Python-free C workflow.

## Tau Encoder 0.2.0 — desktop preview

- Separate Encoder, Decoder preview, and Settings pages.
- Choose Python or experimental Python-free orchestration of the C encoder.
- Keep the pure Rust encoder behind its experimental build feature.

## 0.1.0-alpha.2 — research and specifications

- Include format and codec research, encoder experiment records and measurement data.
- Include archival research scripts and identify their reproduction constraints.
- Keep media-derived clips and vectors out of the public packages.
- No codec runtime changes or new hardware validation claims.

## 0.1.0-alpha.1 — release candidate

- Extract TVC encoder/decoder, TAC reference and fixed-point decoder model, and TNV3 container/film tooling.
- Preserve the validated exact-counting and look-ahead memory improvements.
- Add an opt-in slice-parallel encoder for already sliced pictures, with CPU-slot
  budgeting and serial fallback on worker initialization failure.
- Include synthetic end-to-end smoke tests and format-specification snapshots.
- Target TVC v0 draft 5, TNV3 v0 draft 1.4, TAC v0 draft 1.2 and the TST v0 snapshot.

The pure-Rust crate, full native Linux and Windows validation, and
production-qualified parallel workers remain future work. See `VALIDATION.md`.

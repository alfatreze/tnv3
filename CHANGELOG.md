# Changelog

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

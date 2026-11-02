# Changelog

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

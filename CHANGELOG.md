# Changelog

## 0.1.0-alpha.1 — release candidate

- Extract TVC encoder/decoder, TAC reference and TNV3 container/film tooling.
- Preserve the validated exact-counting and look-ahead memory improvements.
- Add an opt-in slice-parallel encoder for already sliced pictures, with CPU-slot
  budgeting and serial fallback on worker initialization failure.
- Include synthetic end-to-end smoke tests and format-specification snapshots.
- Target TVC v0 draft 5, TNV3 v0 draft 1.4, TAC v0 draft 1.2 and the TST v0 snapshot.

Pure-Rust crate, native Windows/Linux validation and production-qualified parallel
workers remain future work. See `VALIDATION.md`.

# TNV3 codec tools

An alpha host toolkit extracted from Tau V. It contains TVC video, TAC audio and
TNV3 container reference tools; the pure-Rust `tau-video` crate is planned, not
yet implemented. Upstream provenance and MIT notices are retained in
`NOTICE.md` and `LICENSE`.

The default serial encoder and opt-in parallel-slice encoder have passed host
checks on macOS ARM64, and the standalone smoke passes on GitHub-hosted Ubuntu
and macOS runners. Parallel-slice output is identical to serial for the
same slice layout in the tested cases; it remains experimental. Existing
format and decoder evidence is described in `VALIDATION.md`. The hardware
player is maintained in Tau V; this repository's release candidate makes no
new Pocket hardware claim. Full native Linux and Windows gates, long-film, comprehensive
allocation-failure and worker-budget validation remain open.

Next: establish a pinned conformance-vector set with reviewed redistribution
rights, implement the Rust container reader/checker first, then the decoder and
segmented encoder, and validate cross-platform worker integration. See
`docs/REPOSITORY_BOUNDARIES.md` and `docs/RUST_ROADMAP.md`.

Historical codec research, format studies and measurement data accompany the
four specification snapshots; see `docs/RESEARCH_INDEX.md` for their scope and
reproduction constraints. These additions make no new hardware-validation claim.

The research index separates MPEG-4 player history, AV1 subset work, the custom
format family and comparative research; this is a documentation change only.

The README summarizes measured encoder and comparative codec research, with
links to the underlying methods and evidence limits.

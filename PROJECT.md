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


Rust work has started: `tau-video/` reads TNV3 headers and individual index
entries and provides a preliminary info CLI. This is host-tested on macOS; it
is not a full checker, decoder or encoder. The cross-platform CI gate and the
remaining requirements are pending. See `docs/RUST_ROADMAP.md`.

Temporary Tau ν audio support is provided by the optional `tau-mp3` CLI using
FFmpeg: a generated 48 kHz host audio round trip passed on macOS. TAC and
subtitles are future work; this does not add MP3 to the TNV3 format.

The Rust crate now has TVC draft 5 range coding and picture/slice-header
parsing. Host tests match a C-generated range-coder fixture and parse local
reference-vector picture headers. Full picture encode/decode is still pending;
no new Pocket or cross-platform codec claim is made.

TVC integer inverse transforms and intra prediction now match synthetic C
reference fixtures across all quantizers and edge availability cases. This is
host-only component evidence; full frame encoding and decoding remain pending.

TVC motion compensation now has host-tested half-pixel, quarter-pixel luma,
and eighth-pixel chroma paths matching generated C-reference cases. Full frame
codec operation and cross-platform performance remain unverified.

The Rust TVC deblocker now filters full YUV pictures; 104 synthetic pictures
matched the C reference on the host. Full frame encode/decode remains pending;
no new Pocket, Quartus or cross-platform performance claim is made.

The Rust TVC coefficient coder and decoder now match a synthetic C reference
across 300 mixed blocks; this is host-tested component evidence, not a complete
frame codec or new hardware result.

TVC macroblock motion-vector prediction and signed Exp-Golomb coding now match
synthetic C-reference cases on the host. The full macroblock parser and frame
encoder/decoder remain unfinished; no new hardware validation is claimed.

The Rust crate now parses intra-picture macroblocks in a TVC slice, including
adaptive quantizers, prediction modes and 4x4/8x8 coefficients. A generated
C-reference slice passes on the host. P/B syntax, full-frame reconstruction,
encoding and hardware validation remain open.

Rust can reconstruct standalone TVC intra pictures to YUV420; first frames of
21 local vectors match the C decoder byte-for-byte on the host. This does not
include P/B frames, full stream playback or encoding, and makes no new Pocket
hardware claim.

Rust `PictureDecoder` now reconstructs I/P/B TVC pictures in display order.
All 199 frames in the 21 local vectors matched the C decoder on the host; a
synthetic I/P/B sequence is checked in public tests. Full stream/container
validation, seeking, encoding, fuzzing and cross-platform performance remain
open. No new Pocket claim is made.

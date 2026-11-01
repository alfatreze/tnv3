# `tau-video` Rust roadmap

The crate has started in `tau-video/` as a portable, UI-free library with no required C compiler or
external subprocess for core functionality. The separate Omega requirements and
the normative Tau V format specs govern implementation.

1. TNV3 header and index inspection plus info CLI started; complete the checker,
   reviewed, pinned vectors;
   bounded malformed-input tests and stable errors.
2. Bit-exact TVC and TAC decoding, seeking and audio-clock player logic.
3. Segmented encoding, deterministic resume, progress/cancellation and memory
   bounds, cross-checked against the reference tools.
4. Subtitle support and opt-in performance/parallel work.

Each stage requires its own tested release. The C tools remain an independent
cross-check; a full checker, decoder, encoder and native cross-platform validation remain pending.

The immediate audio path is a temporary FFmpeg-backed `tau-mp3` sidecar tool
for Tau ν. It does not change TNV3's TAC format or the library's pure-Rust
constraints. Replace the sidecar path once TAC is finalized and integrated;
subtitle support follows its own format implementation and validation.

TVC groundwork: the Rust range encoder and decoder share a C-reference
bitstream fixture, and picture slice headers are parsed with bounded table
lengths. Macroblock syntax, prediction, transforms, reconstruction, complete
decoding and the encoder remain to be implemented and cross-checked.

The draft 5 inverse 4x4/8x8 transforms and intra prediction are implemented
and tested against generated C-reference results. Macroblock parsing, motion
prediction, deblocking, full decode and encoding still need implementation.

Motion-compensated inter prediction is implemented and compared to generated
C-reference cases; macroblock motion-vector syntax and complete frame decoding
remain future work.

Full-picture luma/chroma deblocking is implemented and matches synthetic C
reference cases. Macroblock parsing, frame decoding and encoding remain open.

Coefficient block entropy encoding and decoding are implemented and checked
against C-generated mixed-block bytes; macroblock syntax and full frame
encoding/decoding still need to connect the existing building blocks.

Motion-vector prediction and signed Exp-Golomb coding are now implemented and
reference-tested; the complete macroblock syntax still needs to combine them
with coefficient coding and picture reconstruction.

Intra-slice macroblock syntax is now decoded and checked against a synthetic
C-reference stream. It does not yet decode P/B macroblocks or reconstruct a
complete frame; the full encoder is also still pending.

Standalone intra-picture reconstruction is connected end-to-end and matches
first frames of 21 local vectors against the C decoder. P/B macroblocks, frame
reordering and complete encoding remain open.

P/B macroblock parsing, reference-picture reconstruction and display ordering
are now implemented. Host comparison matched 199 local reference frames. The
full file/container API, robust malformed-input validation, seeking and encoder
remain open.

A bounded TVC elementary-stream-to-Y4M function and CLI are available. This is
not a complete conformance checker: full level/stream validation, TNV3 decoding,
seeking and encoder implementation remain pending.

A first deterministic intra-only picture encoder is available and cross-checked
against the C decoder. Predictive/B encoding, rate control, stream writing,
resume and the other encoder requirements remain open.

The intra-only baseline also writes bounded TVC1 streams from Y4M with level
selection and QP retries. It does not provide P/B encoding, optimized quality,
TNV3 muxing, progress, cancellation or deterministic resume.

A bounded video-only TVC1-to-TNV3 muxer is now available and checked with the
C container checker. Audio muxing, full container checking and TNV3 playback
remain open. The temporary MP3 path remains separate from the TNV3 format.

TNV3 video-track decoding is available and matched all nine local vectors
against the C demuxer and decoder. Audio, subtitle rendering, full validation
and seeking remain to be implemented.

The stream baseline now supports zero-motion skip P pictures for near-static
frames with a bounded mean-squared error. General motion search, residual P/B
coding, production quality evaluation, cancellation and resume remain open.
The prior all-intra notes above describe earlier checkpoints.

A limited whole-frame translation search now extends the zero-motion P path;
it does not provide block-level search or residual P/B encoding.

Zero-motion residual P candidates are now available for aligned pictures, with
a simple size/error comparison to intra encoding. Nonzero-motion residuals,
block-level search, B encoding and production quality evaluation remain open.

Whole-frame motion and residual P candidates are now combined and checked
against intra reconstruction and size. Block-level search, B pictures and
production quality/performance validation remain open.

An opt-in `block-motion` experiment searches a small per-macroblock window.
It is not production-qualified; wider corpus quality and performance tests and
B-picture encoding remain open. See RUST_ENCODER_EXPERIMENTS.md.

An optional three-frame zero-motion B-group primitive is available for
experiments. It is not integrated with stream encoding or rate control.

The opt-in `encode-tvc-b` stream prototype now emits zero-motion B groups with
residuals and rate-aware intra fallback. It is not competitive on most short
real clips tested and is not production-qualified; see the experiment record.

The opt-in B stream reuses the previous intra anchor, with tested fallback
and partial-group handling. It does not yet share P anchors with the normal
encoder; the short-clip comparisons remain mixed.

The opt-in B stream now shares I/P anchors and refreshes intra pictures at
most 96 display frames apart. It remains experimental after mixed short-clip
results; broader quality, throughput and robustness gates are open.

The opt-in B stream compares each B group against its I/P alternative for size
and decoded error. Short host samples improved the anime trade-off but increased
encoding work; it remains experimental pending broader tests and optimization.

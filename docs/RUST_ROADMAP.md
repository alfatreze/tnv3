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

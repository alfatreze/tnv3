# `tau-video` Rust roadmap

The crate has not been implemented in this release. It is intended to live in
this repository as a portable, UI-free workspace with no required C compiler or
external subprocess for core functionality. The separate Omega requirements and
the normative Tau V format specs govern implementation.

1. TNV3 header/index reader, checker and info CLI; reviewed, pinned vectors;
   bounded malformed-input tests and stable errors.
2. Bit-exact TVC and TAC decoding, seeking and audio-clock player logic.
3. Segmented encoding, deterministic resume, progress/cancellation and memory
   bounds, cross-checked against the reference tools.
4. Subtitle support and opt-in performance/parallel work.

Each stage requires its own tested release. The C tools remain an independent
cross-check; neither a Rust crate nor native cross-platform support is claimed by
this initial source candidate.

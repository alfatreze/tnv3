# Rust encoder experiments

## Block-motion candidate — 2026-10-31

Decision: expose a bounded even-pixel per-macroblock search as the optional
`block-motion` Cargo feature. The normal build keeps it off until broader
quality and performance evidence is available. The candidate uses the existing
size and decoded-error comparison against the intra picture. It affects the
encoder hot path; it does not change the bitstream format or decoder.

**Host:** one generated 160×96, 12-frame, 24 fps case at QP 30. The default
encoded 75,340 bytes at 34.569 dB YUV PSNR; the feature encoded 52,278 bytes
at 35.173 dB. This is a 30.61% size reduction and 0.604 dB PSNR increase on
this one synthetic input, not an equal-quality BD-rate result. Single `/usr/bin/time`
runs reported 0.28 s and 0.27 s wall respectively, too short and noisy to
claim a speed result. C decoder compatibility passed for a separate generated
opposing-motion case. No new Pocket or Quartus claim.

Reproduce the source with `python3 tau-video/tests/reference/block-bench.py
> bench.y4m` (SHA-256 `f48994929fb6e64133a8e76a41064e48cd491de596257e883fa46aed50629d8c`).
Build separately with and without `block-motion`, encode at CLI default QP 30,
decode with the reference and compare size and YUV PSNR. The next gate is a
paired, repeated multi-clip rate/quality and throughput study including motion,
anime, dark scenes and longer inputs.

## Real-clip sample and precheck — 2026-10-31

**Host:** decoded four local reference clips (dialog, anime, black-and-white,
action), scaled each to 400 pixels wide, and tested QP 24, 30 and 36. Each
clip contains only 6–10 frames. Each mode ran twice; the default, ungated
block search and gated block search ran in separate batches. The C decoder
was used for every output; measurements are Y-plane PSNR at the same nominal
QP, not equal-quality BD-rate. See `rust_block_motion_study.json`.

Against the default, ungated block search produced unchanged file size in
7 of 12 cases; the others ranged from 0.70% smaller to 0.56% larger. Y-PSNR
changes ranged from -0.072 to +0.017 dB. The second-run wall-time ratios were
1.05–1.33× default (slower), but these short timings are noisy. This does not
justify enabling the feature by default.

A sampled precheck was then tested inside the optional feature. All 12 gated
outputs were byte-identical to the ungated block outputs; second-run wall times
were 0.869–0.978× ungated, median 0.926×. These are preliminary short,
non-interleaved timings, not a reliable speed claim. The precheck may miss
beneficial blocks outside the sample. The opposing-motion synthetic test still
passes the C decoder. Decision: retain the precheck only in the opt-in feature;
keep the default unchanged. Next gate: longer, interleaved repeated tests and
rate/quality curves over a diverse licensed corpus. No new hardware claim.

## Three-frame B-picture prototype — 2026-10-31

Decision: expose `try_encode_b_group` behind the opt-in `b-pictures` feature.
Two intra anchors are encoded first; the middle picture uses bidirectional
zero-motion skip only if its reconstructed YUV mean squared error is at most
4. Alternatives: emitting B pictures unconditionally would weaken the quality
bound; buffering arbitrary GOPs is deferred until stream and rate handling are
ready. This affects the experimental encoder hot path only when explicitly
called. **Host:** one synthetic three-frame case passed Rust/C decoded-pixel
equality, display ordering and the C TNV3 checker. The B payload was smaller
than a separately encoded intra picture on that case. No performance, corpus,
Pocket or Quartus claim. The stream encoder does not use this function. Next:
rate-window-aware integration, fallback, broader quality tests and benchmarks.

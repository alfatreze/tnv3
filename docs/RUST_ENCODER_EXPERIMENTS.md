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

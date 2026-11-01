# Rust and C encoder parity review — 2026-11-01

**Decision:** retain the C encoder as the mature reference and the Rust crate
as a preliminary independently implemented backend. Target equivalent format
compliance, user-visible options and measured quality/rate/throughput; neither
byte-identical output nor identical decisions are required. The shared app
already uses the same FFmpeg source conversion, audio sidecar and diagnostics
for both modes, but defaults and internals are not equivalent. A direct Rust
binding to C would give the crate mature compression sooner at the cost of a C
build/FFI dependency; it is a viable alternative to porting every algorithm.

**Parity gaps, code review:** C uses segmented multi-job encoding, block and
quarter-pixel motion, I/P/B pictures (two B by default), scene cuts, spatial
intra modes, 4x4 transforms, RDOQ and look-ahead. The normal Rust path is a
streaming I/P encoder with simple intra, zero/whole-frame motion and a
conservative per-picture rate cap; optional block-motion and B prototypes are
not the app default and have not passed quality/performance gates. The QP
setting is therefore not an equal-quality control across encoders. Rust and C
can produce valid but materially different streams and resource demands.
See `issues/001-rust-rate-window-starvation.md` and
`RUST_ENCODER_EXPERIMENTS.md`. The reported Pocket slowdown and C freeze have
not been isolated from source ingest or player behavior; see issues 003/005.

## Unaligned residual improvement

**Host:** the ordinary Rust encoder previously disabled residual P pictures
unless both dimensions were multiples of 16, including 400x226. It now
preserves the actual reconstructed padded reference and evaluates zero-motion
residual P pictures for unaligned frames, applying its existing size, decoded
error and rate-window checks. It keeps the original display size. Tests at
31x19, 32x18 and 400x226 exercise chained residual frames and compare Rust and
C decoded Y4M pixel-for-pixel. This changes the encoder hot path; it does not
port C's block/quarter-pixel search or B pictures.

A generated 24-frame `testsrc2` 400x226 sample at nominal QP 30 gave:

| Standalone encoder | Bytes | I / P / B | Y PSNR | Median seconds (3 runs) |
| --- | ---: | ---: | ---: | ---: |
| Rust before | 193,098 | 24 / 0 / 0 | 41.053 dB | 0.0627 |
| Rust after | 148,118 | 1 / 23 / 0 | 40.981 dB | 0.1087 |
| C reference | 51,194 | 1 / 8 / 15 | 41.961 dB | 0.3318 |

The Rust result is 23.3% smaller at 0.072 dB lower luma PSNR, with a 73% longer
encode time on this tiny synthetic sample. C uses different decisions and QP
behavior. These are same-QP measurements, not equal-quality rate curves or a
reliable comparative speed benchmark; they do not establish Pocket performance.
See `rust_parity_2026-11-01.json` and reproduce with
`tools/codec_study/rust_parity_probe.py`; it generates its own input. No user
media is committed.

## Next gates

1. Establish paired same-ingest, same-input full-app comparisons over diverse
   clips and rate/quality curves, including valid index/rate windows, independent
   decoder pixel checks and repeatable timing/memory measurements.
2. Design production Rust block motion, scene cuts, multi-frame prediction and
   rate control, or integrate the mature C encoder behind a safe Rust API;
   assess maintenance and native toolchain cost before choosing. Do not promote
   the current experimental B feature based on frame count alone.
3. Test encoder/player combinations on Pocket with frame, decoder and audio
   counters; diagnose the observed freeze separately from compression parity.
4. Package and validate both paths on Windows/Linux and validate the desktop UI.

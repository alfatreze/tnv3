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

## B stream integration and residuals — 2026-10-31

The earlier skip-only group is now an opt-in `encode-tvc-b` stream prototype.
It buffers up to three input frames, evaluates skip or zero-motion residual B
pictures, checks decoded error and group size against intra alternatives, and
enforces the video level window. Rejected groups fall back to intra pictures
with QP retries. This changes the experimental encoder hot path only; the
normal P encoder remains the default. **Host:** synthetic groups passed
Rust/C pixel equality, display order, fallback and C TNV3 checker tests.

Four 400-pixel local decoded clips at nominal QP 30, each 6–10 frames, gave
the following B-stream results versus the normal P encoder: dialog +60.85%
bytes / +0.014 dB Y-PSNR; anime +38.85% / -0.153 dB; black-and-white +30.35%
/ +0.084 dB; action -2.53% / +0.136 dB. These are same-QP, short host tests,
not equal-quality rate comparisons. C container checks passed for all four.
The skip-only prototype used no B pictures in these clips; residual coding
used 2–3 per clip but intra anchors outweighed savings on three. Single short
timing runs were too noisy to compare speed. See `rust_b_stream_study.json`.
Decision: retain as opt-in only; no new Pocket or Quartus claim. Next gate:
share anchors with the normal P path, then repeat longer paired quality/rate
and speed studies before considering a default change.

## Shared intra anchors for B stream — 2026-10-31

The experimental B stream now retains the previous intra anchor and buffers
up to two following input frames. It tries a future intra anchor and a B picture
against two intra pictures, with the same error/size and rolling rate checks;
otherwise it writes intra pictures with retries. This affects the opt-in
encoder hot path. Sharing the normal encoder's P anchors is deferred.

**Host:** on the same four short 400-pixel QP 30 clips, the new B stream
changed bytes versus the preceding B prototype by -9.98% dialog, -5.91%
anime, +4.01% black-and-white and +0.07% action. Versus the normal P encoder
it used +44.80%, +30.65%, +35.58% and -2.46% bytes respectively; Y-PSNR
differences were +0.144, -0.199, +0.011 and +0.134 dB. These are same-QP
6–10-frame cases, not equal-quality rate comparisons. Single short timing runs
are insufficient for speed conclusions. The C decoder and TNV3 checker passed
all four; synthetic tests cover display order, fallback, 1–8 frame lengths
and truncated input. Data: `rust_b_stream_study.json`, shared-intra section.
Decision: retain opt-in only; no new Pocket or Quartus claim. Next: share P
anchors and test longer diverse inputs before considering default use.

## Shared P anchors for B stream — 2026-11-01

The experimental B stream now chooses a future I or P anchor using the
existing static, whole-frame motion and residual candidates, and reuses it
for subsequent groups. Failed B groups fall back to I/P anchors in display
order. Intra refresh is bounded to 96 display frames. This changes the opt-in
encoder hot path; the normal stream remains unchanged. **Host:** a 100-frame
synthetic I/P/B stream passed C/Rust pixel equality, C TNV3 checking and
explicit key positions 0 and 96. Four local reference clips at QP 30 also
passed C checking and Rust/C pixel equality.

A short 12-case study (four clips, 6–10 frames each, QP 24/30/36, two runs)
compared the shared-P B path with the normal P encoder. At the same nominal
QP, dialog sizes were 2.15–6.17% smaller and action 3.77–6.04% smaller; anime
was 11.90–12.42% larger and black-and-white ranged from 0.38% smaller to
4.22% larger. Y-PSNR changes ranged from -0.145 to +0.301 dB. These are not
equal-quality rate comparisons. Second-run elapsed-time ratios had a 1.055×
median B/P, but runs were very short and not reliably interleaved; no speed
claim is made. See `rust_b_shared_p_study.json` for measurements.

At QP 30, compared with the earlier shared-intra B prototype, sizes changed
-33.99% dialog, -14.35% anime, -24.63% black-and-white and -1.80% action.
Decision: retain opt-in only; anime regression and limited corpus prevent a
default change. Next gate: longer diverse rate/quality curves, reliable paired
timing, malformed-input tests and broader B-picture search. No new Pocket or
Quartus claim.

## B groups compared with I/P alternatives — 2026-11-01

The opt-in B encoder now builds the two-frame I/P alternative and accepts a B
group only when it is smaller and its combined decoded YUV squared error is no
more than the alternative plus one per sample per frame. The normal P stream
is unchanged. This adds encoder hot-path work and is a local two-frame guard,
not a whole-film guarantee. Non-aligned dimensions currently fall back to I/P.

**Host:** the same four 400-pixel reference clips, QP 24/30/36, 6–10 frames
each, two short non-interleaved runs. Versus normal P at the same QP, dialog
was 2.15–6.17% smaller, anime 0.68–1.70% smaller, black-and-white ranged
from unchanged to 0.46% smaller, and action 3.77–6.04% smaller. Y-PSNR
changes ranged from 0.000 to +0.301 dB. This removes the previous short-sample
anime regression, but is not equal-quality BD-rate. The second-run B/P wall
time ratios had median 1.71×; the clips are too short for a robust speed
claim, though the extra candidate work is material. All 12 B outputs passed
the C TNV3 checker and matched the C decoder pixel-for-pixel in Rust. Synthetic
tests cover I/P/B order, fallback, truncation and key refresh. Raw data is in
`rust_b_guard_study.json`.

Decision: keep opt-in only. Next gate: reduce duplicated candidate work and
measure longer interleaved rate/quality and throughput tests; check whether
local choices improve whole-film behavior. No new Pocket or Quartus claim.

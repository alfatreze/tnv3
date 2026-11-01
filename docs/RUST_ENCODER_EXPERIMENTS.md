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

## Exact static-pair shortcut — 2026-11-01

The opt-in B encoder skips the B trial when both I/P alternatives are exactly
the same zero-motion skip payload. Their references are unchanged and the B
skip payload cannot be smaller under the existing strict size comparison. This
affects the experimental encoder hot path only; normal P encoding is unchanged.
**Host:** output was byte-identical on all 12 earlier short real-clip cases. On
a generated 160×96, 2,400-frame static clip at QP 30, 12 alternating runs had
median wall times 0.070695 s before and 0.049970 s after (0.707×); all outputs
had the same SHA-256. This is a static-scene result, not a general throughput
claim. Input generator: `tau-video/tests/reference/static-bench.py`, SHA-256 of
generated Y4M `230fdd4ec0ba23e4556e463dd89835ec4b16168322fc0314131261dd8f822dc8`.
Raw timings: `rust_b_static_shortcut.json`. Synthetic B fallback and C decoder
tests passed. The B path remains opt-in; wider quality and throughput testing
and a full malformed-input audit remain open. No new Pocket or Quartus claim.

## Reuse a future anchor after skip — 2026-11-01

The opt-in B encoder reuses the already computed future anchor when the middle
picture is exactly a zero-motion skip and the rate window has enough headroom
for any legal picture. The reference and keyframe decision are then unchanged;
otherwise it recomputes the anchor. This affects the experimental encoder hot
path only. A pre-optimization synthetic fixture covers the shortcut.

**Host:** all 12 earlier short real-clip outputs were byte-identical. On one
generated 160×96, 2,400-frame repeat/change sequence at QP 30, eight
alternating runs per mode had median wall times 1.11169 s before and 0.69443 s
after (0.625×); all outputs had the same SHA-256. The C and Rust decoders
matched pixel-for-pixel and the C TNV3 checker accepted the long output. This
is one generated pattern, not a general speed claim. Reproduce input with
`tau-video/tests/reference/reuse-bench.py`; generated Y4M SHA-256:
`05ee066db9e619aaa3190fe6b85163898f86c7b7535cc5157d0788e36e0a37a0`. Raw
timings: `rust_b_anchor_reuse.json`. Default remains unchanged. Wider quality
and throughput tests and malformed-input validation remain open; no new Pocket
or Quartus claim.

## Longer film rate-window gate — 2026-11-01

A 48-frame local film segment was decoded and padded to 400×176 and 800×336,
then encoded at nominal QP 24, 30 and 36 with the normal P and opt-in B paths,
twice each. **Host:** without a per-picture budget, both paths failed at QP 24
at both sizes and QP 30 at 800 pixels because the rolling video-byte window
left too little room for a later picture. See issue 001.

A `safe-rate` Cargo feature caps each picture to floor(window byte limit /
ceil(fps)) and retries higher QPs. All 12 mode/size/QP cases completed and
passed the C TNV3 checker; it required 43–44 retries at QP 24 and 26 retries
at 800/QP 30. It can still fail if a QP-51 picture exceeds the cap. When the
original mode succeeded, results were unchanged at QP 36. At 400/QP 30, the
B result changed from 333,623 bytes / 37.3135 dB Y-PSNR to 305,611 bytes /
36.3222 dB, demonstrating a substantial trade-off. Same-QP PSNR is not an
equal-quality rate comparison; see `rust_long_rate_study.json`.

Decision: retain the conservative budget as opt-in, normal behavior unchanged.
It is a workaround, not look-ahead rate control; test longer and more diverse
inputs, rate/quality curves and audio headroom before any default decision.
This affects the encoder hot path when enabled; no new Pocket/Quartus claim.

## Relaxed rate budget — 2026-11-01

A scratch-only experiment raised the conservative per-picture budget to 1.5
times its equal-share value while retaining the rolling-window check. On the
same two 48-frame film segments, **host** runs for P and B at QP 24 failed at
both sizes, and at QP 30 failed at 800 pixels, in both repetitions. At 400/QP
30 it recovered the original B result: 333,623 bytes and 37.3135 dB Y-PSNR,
versus 305,611 bytes and 36.3222 dB for the conservative option. Same nominal
QP does not establish equal-rate quality. The 1.5× budget is not a safe
replacement: it does not reserve enough space in subsequent windows. Results
and timings are in `rust_long_rate_study.json`; film inputs remain local.
The experiment changed the encoder hot path in scratch only and was rejected,
not shipped. The next gate remains bounded look-ahead or segment-aware rate
allocation with tests across window boundaries; no hardware claim is made.

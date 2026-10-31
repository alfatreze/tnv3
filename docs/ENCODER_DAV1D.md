# dav1d approaches applicable to the TVC encoder

E-020 applies the specialized-phase/interior-kernel approach to TVC quarter-pel
search with exact pixels/streams and measured host speed gains. See
[ENCODER_QPEL.md](ENCODER_QPEL.md); no dav1d source or AV1 filter is imported.

## Assessment (E-018, 2026-10-30)

dav1d is VideoLAN's AV1 decoder. Its engineering patterns are useful to this
CPU-heavy encoder; its AV1 filters, transforms, syntax and entropy state are
not TVC implementations. This review imports no source and claims no measured
new Tau speed/compression gain. Production decoder/RTL/firmware remain frozen.

Sources reviewed were VideoLAN's public repository master files on this date.
These are moving references, not a pinned vendored dependency. A future source
import must pin a revision and preserve the relevant file's notices.

## Ranked adaptations

| Priority | Observed dav1d approach | TVC application / gate |
|---|---|---|
| 1 | Generic reference functions, optimized variants and checkasm correctness/benchmark coverage | Expand our existing SAD/bit-cost differential checks to whole fractional-prediction output, borders, phase combinations, odd strides and extreme samples. Compare pixels and ABI/memory safety, then full streams. |
| 2 | Specialized motion-compensation kernels and separate edge emulation | Prototype encoder-only interior quarter-pixel kernels; preserve TVC's four-tap coefficients, clipping and rounding exactly, with the frozen generic implementation as reference/fallback. |
| 3 | DSP function tables and CPU feature masks | Introduce a small encoder DSP interface when multiple beneficial variants exist; select once per encoder/worker. Only select measured faster supported code. Keep C fallback and forceable feature masks. |
| 4 | Explicit picture allocation/lifetime and dependency-aware worker tasks | Consider private encoder contexts and independent non-reference B-picture work after immutable anchors exist. Keep decoding-order output and level budgeting deterministic. |
| 5 | Explicit alignment/padding and reusable picture storage | Reuse encoder scratch/work buffers and future worker reference buffers; measure cache/allocation effects. Preserve existing TVC coded dimensions, strides and clamps. |

These are recommendations inferred from source review. Their benefit in Tau
has not been benchmarked. Existing optional NEON/SSE2 paths already demonstrated
why instruction availability alone is insufficient: Apple Silicon measurements
were mixed and the portable compiler-vectorized path stayed the default.

## First concrete implementation candidate

Profile actual encoding-stage fractional prediction, coefficient counting,
transforms and intra evaluation before choosing the next expensive kernel.
The earlier coarse phase profile only establishes that look-ahead is a small
share; it does not identify the dominant function inside main encoding.

If fractional prediction warrants it, create encoder-only kernels under
`tools/tvc_ref/enc/` with separate integer, horizontal, vertical and 2-D phase
paths. Our integer in-bounds SAD fast path is already accepted. For fractional
interior blocks, avoid redundant coordinate clamps while retaining the generic
border path. TVC's 2-D luma filtering keeps horizontal intermediates unrounded
and unclipped and rounds/clips after the vertical pass; an optimized path must
reproduce that exact arithmetic, including negative vectors. Chroma's rules
also stay TVC-specific.

Suggested gates, in order:

1. Per-pixel differential tests against frozen C for every phase, border,
   negative displacement, unaligned address and worst-case sample pattern;
   native sanitizers and forced-C/ISA variants.
2. Kernel microbenchmark followed by alternating whole-encoder benchmarks on
   representative clips. A kernel improvement alone is not acceptance.
3. Existing full corpus gate, frozen vectors, reconstruction match and parser
   cost comparison; paired stream hashes for intended exact speed changes.
4. Level-enforced full film path, `tnv3 check`, and native platform evidence.

Do not add a runtime indirect call in the innermost pixel loop merely to copy
the shape of another project. Dispatch at a coarse kernel boundary and measure
the overhead. AVX paths need CPU and OS state support checks; compile-time SSE2
or NEON availability is not a substitute for that dispatch design.

## Threading is a separate architectural change

dav1d's task scheduler tracks dependencies and progress. TVC encoding currently
uses process-global picture, context, source/current/reference and rate-window
state. Copying a worker loop into it would introduce races. A future design
needs per-worker state, explicit reference lifetimes, scratch ownership,
deterministic candidate tie-breaking and ordered output/rate-window commit.

Non-reference B pictures are plausible parallel work after their two anchors
are reconstructed. Level enforcement currently raises picture QPs based on
earlier picture lengths, so parallel analysis/encoding must still honor that
ordered budget, including deterministic re-encodes. Adding more slices to
manufacture independence can cost rate and is not a free speed optimization.
The existing segment-level parallel film path remains available meanwhile.

## What stays outside this adaptation

- AV1 interpolation coefficients, inverse transforms, probability/CDF update
  rules, film-grain synthesis and reconstruction tools cannot replace TVC's
  normative operations.
- dav1d is a decoder; it does not supply encoder motion-search, quantization
  or rate-distortion strategy for us to transplant.
- Its picture allocator's alignment/padding contract is not a new TVC stride
  contract. Extra scratch padding must remain encoder-private.
- Replacing or modifying frozen `tvc.h`, `tvc_dec.c`, spec, RTL or firmware
  remains outside this branch. An encoder-only copy of a needed routine can
  be optimized while the frozen routine remains the validation reference.

## Provenance and primary references

No dav1d code is copied in this change. The reviewed COPYING file contains the
BSD two-condition redistribution terms, including source/binary notice duties;
any later import needs its applicable license and author notices retained.
No blanket licensing-compatibility conclusion is made here.

- [VideoLAN project overview](https://www.videolan.org/projects/dav1d.html)
- [CPU initialization and masks](https://github.com/videolan/dav1d/blob/master/src/cpu.c)
- [x86 CPU/OS feature checks](https://github.com/videolan/dav1d/blob/master/src/x86/cpu.c)
- [ARM motion-compensation dispatch](https://github.com/videolan/dav1d/blob/master/src/arm/mc.h)
- [x86 motion-compensation dispatch](https://github.com/videolan/dav1d/blob/master/src/x86/mc.h)
- [Generic motion compensation and edge emulation](https://github.com/videolan/dav1d/blob/master/src/mc_tmpl.c)
- [checkasm CPU variants](https://github.com/videolan/dav1d/blob/master/tests/checkasm/checkasm.c)
- [Motion-compensation reference/optimized comparisons and benchmarks](https://github.com/videolan/dav1d/blob/master/tests/checkasm/mc.c)
- [Picture allocation contract](https://github.com/videolan/dav1d/blob/master/include/dav1d/picture.h)
- [Picture lifetime implementation](https://github.com/videolan/dav1d/blob/master/src/picture.c)
- [Dependency-aware task scheduling](https://github.com/videolan/dav1d/blob/master/src/thread_task.c)
- [Arithmetic decoder implementation](https://github.com/videolan/dav1d/blob/master/src/msac.c)
- [COPYING](https://github.com/videolan/dav1d/blob/master/COPYING)

All adaptation gains and native Windows/Linux dispatch/threading behavior are
pending experiments. Existing E-015 production gates and later feasibility
records remain the measured state; this document adds a code-review plan only.

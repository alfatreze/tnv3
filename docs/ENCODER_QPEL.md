# Exact fractional motion-search speedup (E-019/E-020)

The following E-022 step adds exact RDOQ prefix reuse. Its later measurements
are in [ENCODER_RDOQ_PREFIX.md](ENCODER_RDOQ_PREFIX.md); this document retains
the isolated fractional-kernel result and comparison.

## Decision

Use encoder-private, phase-specific 16x16 luma prediction for interior search
blocks. Keep the unchanged reference prediction for border footprints. The
existing integer SAD fast path stays in place. No source is imported from
dav1d; its separation of generic/reference kernels, specialized phases and
edge handling informed the engineering approach reviewed in E-018.

The implementation is `tools/tvc_ref/enc/qpel.h`; only the encoder's SAD search
calls it. Reconstruction, decoder, bitstream, RTL, firmware and shared `tvc.h`
are unchanged. It reads the existing frozen `TVC_QF` coefficients. Horizontal
and vertical cases use the same final rounding/clipping. In the 2-D case,
19 rows of horizontal intermediates are calculated once and reused by the
vertical pass, with no intermediate clipping or rounding. Negative vectors
keep the same shifts/remainders as the reference. Maximum intermediate bounds
fit signed 32-bit arithmetic (horizontal magnitude <=18360; a conservative
vertical bound is 80*18360 before final rounding).

Alternatives: AV1 filters are semantically incompatible; explicit ISA assembly
is deferred until generic code is measured; changing reference-frame padding
or strides would add ownership/edge complexity. The interior-only path with a
reference border fallback is the smallest exact implementation. The 2-D scratch
is 19x16 integers on the call stack; no per-call heap allocation is introduced.

## Profiling evidence and limits

Before this optimization, temporary function timers on source revision
`2ba43ad0` measured fractional search prediction plus SAD at about 19–26% of
instrumented runtime on action_400, anime_dialog_400 and action_800 (96 frames,
QP30). RDOQ accounted for roughly 33–50%. `fdct8` timings exclude the manually
implemented 4x4 transforms. Timers and calls affect optimization and add
overhead, so these are coarse hotspot indicators, not unbiased stage fractions
or throughput estimates. Source hash and call counts are in
`docs/encoder_hotspots.json`.

Reproduce from the same source revision using the optional POSIX research tool:

```sh
git show 2ba43ad0:tools/tvc_ref/tvc_enc.c > work/encoder_lab/hotspots-base.c
python3 tools/tvc_ref/enc/profile_hotspots.py --source work/encoder_lab/hotspots-base.c --out work/encoder_lab/hotspots.json
```

## Correctness

- 30720 whole-pixel differential cases match the frozen predictor, covering
  all 16 phases, negative/positive displacements, 80x64/37x41/16x16 reference
  sizes, zero/full/alternating/random/impulse samples, unaligned input and
  guarded odd-stride output. 26960 cases use the border fallback; rejected
  footprints leave output untouched.
- Native ARM64 address/undefined-behaviour sanitizers pass. Rosetta x86-64
  pixel comparisons pass. Existing 20000 half/qpel SAD comparisons pass.
- Full native O3 and default O2 gates pass: frozen vectors, decode/reconstruction equality,
  path guard and parser-cost acceptance. Corpus bytes, PSNR and bins remain
  exactly the accepted E-007 result. Evidence: `docs/encoder_qpel.json`,
  `docs/encoder_qpel_o2.json` and work/encoder_lab's qpel gate logs.

```sh
cc -O3 -fsanitize=address,undefined -o work/encoder_lab/check-qpel tools/tvc_ref/enc/check_qpel.c -lm
work/encoder_lab/check-qpel
clang -arch x86_64 -O3 -o work/encoder_lab/check-qpel-x86 tools/tvc_ref/enc/check_qpel.c -lm
work/encoder_lab/check-qpel-x86
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py gate
```

## Whole-encoder performance

Three alternating before/after repetitions, 96 frames, Apple Clang O3 on this
M4 Pro; no concurrent benchmark/build. Before is the already optimized E-018
CPU fast encoder. Every paired stream SHA256 is identical.

| case | before fps | after fps | throughput ratio |
|---|---:|---:|---:|
| action 400, QP30 | 39.04 | 45.59 | 1.1677x |
| anime_dialog 400, QP24 | 17.21 | 18.27 | 1.0613x |
| bw_room 400, QP36 | 39.80 | 52.48 | 1.3187x |
| action 800, QP30 | 14.71 | 18.76 | 1.2751x |

Evidence: `docs/encoder_speed_qpel.json` and
`docs/encoder_speed_qpel_800.json`. These are native host measurements, not
Windows/Linux/native Intel speed claims or a real-time guarantee for all clips.

## Film-path validation

The complete 240-frame action study clip plus silent audio passes the full
`encode_film.py` and explicit `tnv3 check` paths at QP30, three segments and
two workers:

- 400x166, level1: output is byte-identical to E-007's full TNV3 file.
- 800x334, level2: before/after full TNV3 outputs are byte-identical, 976528 B,
  max picture 25055 B, worst second 160475 B. No level QP raises or joint
  re-encodes. Reported one-run full-pipeline times round to 11 s before and
  10 s after; these rounded integration times are not repeated benchmarks.

The 800 input was losslessly wrapped from `action_800.y4m` with FFV1 and silent
48 kHz stereo PCM. Reference TAC/TNV3 tools were compiled into work/ and supplied
to the film module, as in earlier validation. Reports are
`docs/encoder_film_qpel.json`, `encoder_film_qpel_800_before.json` and
`encoder_film_qpel_800.json`; logs and binaries are under work/encoder_lab.

No full-length film, subjective quality study, stressed rate cap, level3 film,
native Windows/Linux run, RTL/Quartus fit/timing or Pocket measurement was
performed. No new hardware resource/timing figure is claimed. Further work
should re-profile the main stage after this gain before optimizing RDOQ or
adding parallel workers. The CPU fast binary remains O3; default lab builds
remain O2 unless `TVC_ENCODER_CFLAGS=-O3` is supplied.

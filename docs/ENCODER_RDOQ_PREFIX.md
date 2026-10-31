# Exact RDOQ prefix-state reuse (E-021/E-022)

## Decision and invariants

Keep the existing greedy quantization decisions and coefficient model. Replace
repeated full coefficient-count replays with an encoder-private prefix counter
in `tools/tvc_ref/enc/rdoq.h`. This changes CPU analysis work only. It is not
trellis quantization, a new probability model, a new syntax, or an actual-byte
rate estimator: the floating-point RC_CNT cost approximation remains the same.

The coefficient syntax is processed from high scan positions downward. At each
trial, higher chosen coefficients are already fixed. Their adapted class models,
greater-than-one count and accumulated floating-point cost can be reused before
counting the current coefficient and remaining suffix. The accumulator resumes
with the same value and addition order as a fresh replay; counting suffix costs
separately and adding them afterwards could change rounding and is avoided.

| trial | treatment |
|---|---|
| Lower magnitude, last position unchanged | Copy prefix models/cost, replay only the remaining suffix |
| Remove a non-last coefficient | Recount its significance and the full suffix, including changed neighbor classes |
| Remove the last coefficient | Use the unchanged full-block counter; if accepted, reset prefix/header/models from the original live class |
| Sole magnitude-one coefficient | Keep it, as the old algorithm does; the caller separately evaluates omitting the entire block |
| Sole larger coefficient | Perform its one possible trial with full replay, avoiding unnecessary prefix setup |

After each decision, advance the retained prefix by the actual chosen symbol.
The trial's final state cannot become the next prefix because it already
contains later suffix symbols. Original live contexts are never mutated.
The implementation copies only the selected coefficient-class model arrays.
Nonzero quantized blocks are the production precondition; an empty input also
returns zero safely. The 8x8 luma/chroma and 4x4 luma paths are covered.

Alternatives: changing decisions/quantization is unnecessary for this speed
step; retaining state after a last-position change is incorrect because the
header and implicit first significant coefficient change; independent suffix
cost addition risks floating-point differences. Full replay stays the oracle
and fallback. Shared `tvc.h`, decoder, firmware and RTL remain unchanged.

## Profiling and correctness

Post-E-020 coarse profiles indicate RDOQ at about 54–58% of instrumented runtime
on the 400-pixel QP30 samples and 41% on action_800. Fractional prediction/SAD
fell to roughly 6–7%. These include timing-call overhead and compiler effects;
`fdct8` excludes manual 4x4 transforms. Source revision/hash and raw figures are
in `docs/encoder_hotspots_post_qpel.json`. The profiling tool now handles both
the old function and the new wrapper around the private implementation.

`check_rdoq.c` retains the pre-prefix algorithm as a differential oracle. Its
12000 synthetic blocks span all QPs 0..51, each legal class/size, sparse/dense
and trailing-one patterns, singleton and large levels, and randomized valid
probability states. It compares all output levels/counts, verifies that original
contexts do not change, and enables `TVC_VERIFY_RDOQ` to check cached trial cost
against a fresh full replay plus the final prefix against the chosen cost.
Native ARM64 address/undefined-behaviour sanitizers and Rosetta x86-64 pass.

```sh
cc -O2 -fsanitize=address,undefined -o work/encoder_lab/check-rdoq tools/tvc_ref/enc/check_rdoq.c -lm
work/encoder_lab/check-rdoq
clang -arch x86_64 -O3 -o work/encoder_lab/check-rdoq-x86 tools/tvc_ref/enc/check_rdoq.c -lm
work/encoder_lab/check-rdoq-x86
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py gate
python3 tools/encoder_lab.py gate
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py build
```

Both O3 and default O2 full gates pass with exactly the previous corpus sizes,
PSNR and syntax-bin counts. Encoder/decoder reconstructions and frozen vectors
pass. Evidence: `docs/encoder_rdoq_prefix.json`, `encoder_rdoq_prefix_o2.json`
and work/encoder_lab's rdoq-prefix gate logs. Release builds omit the extra
full-replay debug assertions.

## Whole-encoder timing

Three alternating repetitions per case, 96 frames, native M4 Pro, Apple Clang
O3. Before is the accepted E-020 encoder. Every paired stream SHA256 matches.

| case | before fps | after fps | throughput ratio |
|---|---:|---:|---:|
| action 400, QP30 | 46.90 | 52.11 | 1.1110x |
| anime_dialog 400, QP24 | 18.81 | 21.59 | 1.1475x |
| bw_room 400, QP36 | 53.53 | 57.20 | 1.0687x |
| action 800, QP30 | 18.67 | 20.27 | 1.0860x |

Raw data and binary hashes: `docs/encoder_speed_rdoq_prefix.json` and
`encoder_speed_rdoq_prefix_800.json`. These are measured additional gains on
this host, not universal CPU/platform figures. Native Intel/AMD, Linux and
Windows timing remain unverified.

## Film-path evidence and handoff

Both complete 240-frame study-clip integrations, with silent audio, QP30, three
segments and two workers, pass `encode_film.py` and explicit `tnv3 check`:

- 400x166 level1: complete output byte-identical to E-007's file, 447692 bytes.
- 800x334 level2: complete output byte-identical to E-020's file, 976528 bytes.
  Largest picture 25055 B, worst second 160475 B; no QP raises or joint retries.

Reports are `encoder_film_rdoq_prefix.json` and
`encoder_film_rdoq_prefix_800.json`. Rounded one-run integration times are not
used as repeated performance evidence. No full-length movie, stressed rate
cap, level3 integration, subjective study, new FPGA fit/timing or Pocket test
was performed. No hardware resource/timing change is claimed; the bitstream
and decoder cost evidence stay unchanged.

The current encoder remains the native O3 CPU fast build. Re-profile the
remaining counting/model-copy work before another optimization; asynchronous
workers and alternate syntax/quantization decisions are separate changes.

## E-029: omit unused suffix model copies

Suffix-only trials do not encode a last-position header, so they need only the
significance, greater-than-one and remainder models. The trial copy now skips
`last`; full replay and prefix initialization still copy it. The counting
order and floating-point accumulator remain unchanged. The 12,000-block
full-replay differential test passes with address and undefined-behavior
sanitizers. Full O3 corpus gate passes with unchanged rate, decoder bins and
reconstruction. A 240-frame level-2 study film and frozen decoder output match
the previous TNV3 file byte-for-byte.

Five alternating paired host repetitions show speed ratios 1.009 action400,
1.016 anime400 and 1.010 bw400; three at action800 show 1.003. All output
hashes match. The 800-pixel result and the small 400-pixel changes may include
measurement noise; no large gain or quality change is claimed. Raw data:
`encoder_speed_suffix_copy_repeat.json`, `encoder_speed_suffix_copy_800.json`;
initial three-run evidence is `encoder_speed_suffix_copy.json`. Host M4 only;
no native Windows/Linux performance measurement or Pocket impact.

## E-034: count-only RDOQ symbol helpers

The prefix/header and suffix-symbol paths always use `RC_CNT`. Encoder-private
helpers now perform exactly the count-mode probability lookup, accumulation and
update without the decoder/encoder mode dispatch. Exp-Golomb prefix updates and
each bypass-bit `+1.0` retain their original order; the sign bypass also adds
`1.0`. Full replay still calls the frozen shared counter and is the differential
oracle. This changes CPU analysis, not the bitstream, quantizer or decoder.

12,000 differential blocks pass with per-trial exact cost assertions under
ASan/UBSan and on x86-64 under Rosetta. O3 and default O2 corpus gates pass,
with identical bytes, PSNR and bin counts to the previous encoder. Three paired
O3 host repetitions with exact output hashes: action400 52.48->60.83 fps
(1.159x), anime400 22.00->29.66 (1.348x), bw400 57.64->63.25 (1.097x),
action800 20.39->22.35 (1.096x). Raw: encoder_speed_count_special*.json.
A 240-frame 800x334 level-2 film plus silence passes TNV3 and matches the
previous complete output byte-for-byte (encoder_film_count_special.json).
No native Windows/Linux timing, full-length film or energy measurement.

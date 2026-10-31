# Encoder optimization results and remaining gates

Host measurements, 2026-10-31; encoder-lab worktree. The initial comparison
encoder is the frozen `tvc-dec-draft5` encoder built with `-O2`; current
production is built `-O3`. The comparison includes compiler and algorithm
changes. Identical clips, frame counts, nominal QP and machine were used. Since
E-007 changes encoder choices, original and current streams differ; same-QP
speed is not an equal-quality speed comparison.

## Total throughput

Three alternating paired 96-frame repetitions, median frames/s:

| Study case | Original serial O2 | Current serial O3 | Serial ratio | Experimental worker O3 | Worker ratio vs original |
|---|---:|---:|---:|---:|---:|
| action400 QP30 | 35.13 | 60.39 | 1.719x | 88.43 | 2.480x |
| anime_dialog400 QP24 | 16.03 | 29.46 | 1.838x | 40.24 | 2.490x |
| bw_room400 QP36 | 28.86 | 63.09 | 2.186x | 86.74 | 2.988x |

The worker ratios are separately paired original-vs-worker runs (their original
medians vary slightly from the serial pair). The worker uses additional cores,
is a generated research variant and is not the default. At action800 QP30,
original-to-current serial is 11.68→22.65 fps (1.939x); a separate current
serial-to-worker pair is 22.61→33.27 fps (1.472x) with identical streams. Raw
individual samples, binary and stream hashes: `encoder_final_speed_total.json`,
`encoder_final_speed_total_bsafe.json`, `encoder_final_speed_total_800.json`,
`encoder_final_speed_bsafe_800.json`. Earlier single-run 240-frame comparisons
and peak memory are in `encoder_long_baseline.json`.

## Quality and decoder work

Final production gate (`encoder_final_gate.json`):

| Clip | BD-rate vs original | Equal-quality decoder-bin change |
|---|---:|---:|
| dialog | −0.92% | −0.31% |
| action | −1.58% | −0.74% |
| anime_dialog | −2.45% | −1.88% |
| bw_room | −1.13% | −0.74% |
| Mean BD-rate | −1.52% | — |

Mean bins/byte 9.676 vs original 9.618; mean bins/macroblock 51.87 vs 52.13.
Equal-quality bin changes use the same three-point interpolation as the gate;
they are not hardware cycle measurements. The gate also confirms each encoder
reconstruction matches the frozen decoder, frozen vectors and parser model pass,
and encoding without reconstruction produces the same streams.

The last accepted speed optimization specializes count-only RDOQ operations:
1.10–1.35x over its immediate predecessor in 400-pixel tests and 1.096x at 800,
with identical outputs (E-034). Other exact improvements and their paired data
are retained chronologically in `ENCODER_LOG.md`. The two-buffer propagation
change reduces that allocation from n to two frame buffers (E-037). Releasing
completed look-ahead arrays reduced measured 800-pixel peak RSS with full
reconstruction from 215.27 to 204.61 MiB in one 240-frame trial; small clips
were mixed (E-038). Total memory still scales with input length.

## Validation and limits

Final 400x166 level1 and 800x334 level2 film-path checks each cover 240 action
study frames and silence, three segments. Both pass TNV3 checking and their
complete files match earlier accepted outputs: `encoder_final_films.json`.
Final host checks also pass: 12000 RDOQ differential blocks under ASan/UBSan,
200 randomized look-ahead sequences under ASan/UBSan, pool reuse 200000 tasks,
spatial metric and CPU-budget boundary tests, and the encoder path guard.
The experimental explicit-context worker has passed its corpus, rate-retry,
film, bounded ThreadSanitizer and selected injected-failure tests (E-041,
E-046), but lacks complete allocation-failure handling and production review.

NEON was not faster in the post-E-034 host measurements and remains opt-in;
x86-64 SSE2 gates passed under Rosetta, not native Intel/AMD (E-036). Windows
worker logic passed a pthread-backed model only; native Windows and Linux
build/runtime tests are outstanding (E-043). VideoToolbox motion seeds and
hardware ingestion did not improve these measurements and remain disabled
(E-024/E-026). GPU/ANE speed, power, thermal behavior, authentic full-length
films, 4K/HDR/10-bit input and new Pocket or FPGA timing evidence were not
measured. The user's player-padding explanation for a prior visible band remains
a hypothesis, separate from codec luma metrics (E-030).

All tested global AQ/tree changes failed their gates. Three B pictures had
better mean rate but increased equal-quality decoder work on two clips (E-045).
B-QP offset 3 had a negligible standard-corpus gain and increased estimated
bins on a holdout clip, so it was not promoted (E-047). These are not subjective
quality studies or an exhaustive search of encoder decisions.

The film helper provides an opt-in encoder-slot budget (E-039), covering TVC
jobs only. It is not an automatic system-wide thread or memory scheduler.
A portable, manually reviewed worker integration, complete allocation-failure
handling, native-platform checks, longer diverse films and bounded whole-input
memory remain open. The Windows research backend is modeled, not validated on
Windows. No claim is made that every possible optimization has been exhausted.

Subsequent parallel-hypothesis tests (E-049–E-052) include four-worker B4/B6,
parallel existing slices, parallel look-ahead, within-picture motion searches,
and longer repeated-input pipeline scheduling. See
[ENCODER_PARALLEL_HYPOTHESES.md](ENCODER_PARALLEL_HYPOTHESES.md). They are
research results and do not change the production totals above.

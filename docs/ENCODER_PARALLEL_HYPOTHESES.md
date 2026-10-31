# Parallelism hypotheses: measured results (2026-10-31)

Host: Apple M4 Pro, macOS ARM64, O3, frozen TVC decoder. The encoder worker,
slice, look-ahead and motion variants are **generated research binaries**;
production remains serial. All FPS figures below use whole-encoder wall time,
not kernel-only estimates. Paired timing results have three alternating runs
unless stated otherwise. Threads consume additional CPU cores. No energy,
thermal, native Windows/Linux or Pocket timing claim is made.

## More B pictures and workers

The current encoder uses two B pictures between anchors. Against that accepted
encoder, four B pictures change BD-rate by dialog −10.67%, action +1.74%,
anime +3.79%, B&W −7.35% (mean −3.12%), but increase equal-quality decoder bins
on action by 3.59% and anime by 5.66%. Six B pictures change BD-rate by
−12.58%, +5.31%, +10.92%, −9.23% respectively (mean −1.40%), and increase
bins on action by 7.87%, anime by 13.68% and B&W by 2.17%. Both fail the
quality/cost gate despite benefits on quieter clips. Three B pictures were
already measured in E-045; they also fail decoder cost on action/anime.
These are three-QP, 96-frame PSNR interpolations, not subjective quality.
Raw gate results: `encoder_bcount_4.json`, `encoder_bcount_6.json`.

The prior pool handles at most two pictures concurrently. A separate `bwide`
prototype adds a four-worker pool and batches up to four B pictures. Same
B-count, same-input streams are byte-identical:

| Clip, 48 frames | B count | Two workers | Four workers | Ratio |
|---|---:|---:|---:|---:|
| action400 QP30 | 4 | 83.66 fps | 119.40 fps | 1.427x |
| anime400 QP24 | 4 | 39.45 fps | 52.92 fps | 1.341x |
| action400 QP30 | 6 | 85.81 fps | 114.64 fps | 1.336x |
| anime400 QP24 | 6 | 41.68 fps | 53.24 fps | 1.277x |
| action800 QP30 | 6 | 32.01 fps | 42.90 fps | 1.340x |

`encoder_speed_bwide_*_final.json` contains individual paired timings and
hashes. `encoder_speed_bgroups_{2,4,6}.json` measures two workers versus serial
for each group size. The four-worker speedup does not repair B4/B6 quality or
decoder cost. The existing two-slot film budget does not support this variant;
when `TVC_B_WORKERS` is set to a value other than 4 the final `bwide` binary
falls back to serial. Unset selects four for this research experiment.

B4/B6 tight level1 retry and B6 level3 stress cases match serial streams and
frozen-decoder reconstruction. Injected job-preparation failures at each of
four slots match serial; a bounded B6 ThreadSanitizer encode passes. A 240-frame
level1 action stream with B6 matches serial reconstruction and stream and passes
TNV3 checking (`encoder_bwide_level1_study.json`); it has no audio. Native
Windows is simulated only. `encoder_bwide_stress.json` and
`encoder_experiment_bwide_parallel.json` preserve the cases and binary identity.
The stress/TSan check was rerun after adding the worker-limit guard.

## Parallel slice encoding

Independent slice contexts can be encoded concurrently, then joined in original
order before the existing deblock. `bslice` does this with two experimental workers,
private picture/coder state and shared disjoint slice rows. For a fixed slice
layout, all paired output hashes are identical:

| Existing slice layout | Serial | Two workers | Ratio |
|---|---:|---:|---:|
| action400 QP30, 3 MB rows/slice | 60.47 fps | 102.50 fps | 1.695x |
| anime400 QP24, 3 rows | 29.56 fps | 48.13 fps | 1.628x |
| bw400 QP36, 3 rows | 62.57 fps | 108.57 fps | 1.735x |
| action800 QP30, 7 rows | 22.38 fps | 31.43 fps | 1.404x |
| action800 QP30, level3 automatic two slices | 22.35 fps | 40.72 fps | 1.822x |

Do not introduce extra slices merely for this speed: using three rows per slice
on the four standard 400-pixel clips increases mean BD-rate by **9.26% versus
current**, with a +0.76% equal-quality bin change on action. Both serial and
parallel sliced gates fail against the unsliced baseline in the same way.
Where level3 already requires two slices, parallelizing the existing layout
avoids this extra coding change; it still needs production integration and
platform testing. The 240-frame level3 action study stream matches serial and
the frozen decoder, and a video-only TNV3 container checks valid
(`encoder_slice_level3_study.json`). Five level/AQ/slice stress cases and a
12-frame sliced ThreadSanitizer run pass. This is not a full-length film, an
actual 800x720 source, or comprehensive race/failure evidence.

## Look-ahead and within-picture candidate work

The fresh phase profile measures source look-ahead analysis at 1.3–2.2% of total
runtime for four QP30 study cases. Its approximate impossible-perfect-speedup
ceiling is 1.013–1.022x; timers and build perturbation apply. A `blook`
prototype divides frame analysis between two workers and retains serial backward
propagation. Five paired runs measured 1.016–1.020x on three 400-pixel cases;
three runs measured 1.011x on action800. Streams match; full corpus and five
stress cases pass; a 24-frame ThreadSanitizer run and 240-frame level2 film
comparison pass. These small end-to-end differences are sensitive to noise and
use extra cores. Evidence: `encoder_parallel_phase_profile.json`,
`encoder_speed_look_parallel*.json`, `encoder_look_parallel_gate.json`,
`encoder_look_parallel_stress.json`, `encoder_film_blook_parallel.json`.
The phase profile covers `look_costs`, not every possible look-ahead operation.

A `bmotion` prototype runs list-0 and list-1 motion searches concurrently for
B macroblocks, waiting before mode decisions. On 48-frame action and anime it
was 0.933x and 0.970x serial speed, respectively; streams were identical.
A bounded 12-frame ThreadSanitizer/reconstruction check passes. Fine-grained
submission/synchronization and extra CPU are plausible costs, but the timing
does not independently attribute the slowdown. Rejected: no full corpus or film
acceptance and no default change. Evidence `encoder_motion_parallel_speed.json`.

## Segment and pipeline scheduling

The film helper overlaps source ingestion with segment encodes and schedules
audio processing. For a **synthetic 960-frame file made by repeating the same
10-second clip four times**, two alternating runs per configuration produced:

| Segment length | Encoder-slot budget | Threads/job | Max jobs | Median total time |
|---:|---:|---:|---:|---:|
| 96 | 2 | 1 | 2 | 21.79 s |
| 96 | 4 | 1 | 4 | 18.63 s |
| 96 | 4 | 2 | 2 | 21.59 s |
| 480 | 2 | 1 | 2 | 24.39 s |
| 480 | 4 | 1 | 4 | 17.18 s |
| 480 | 4 | 2 | 2 | 22.58 s |

For a given segment length all complete TNV3 files match across configurations
and pass checking; 96- and 480-frame segmentation produce different files, so
their times are not a quality-equivalent comparison. With 480-frame segments
there are only two video jobs; a larger executor also allows audio to occupy a
separate slot. The budget only limits TVC slots, not ffmpeg/audio/system CPUs.
`encoder_pipeline_repeated.json` contains every timing and report. Results are
not generalizable full-film or CPU/power conclusions.

Using one serial encoder job, two runs of the existing overlapping pipeline
averaged/median 31.31 s versus 30.83 s for a deliberately synchronous executor;
all outputs match (`encoder_pipeline_overlap.json`). This tiny reversal is not
credible evidence that overlap is harmful: source decoding and silent audio are
cheap here, two samples are insufficient, and a real compressed input may differ.
The synchronous mode also changes audio scheduling, so this is a whole-pipeline
comparison rather than an isolated decoder benchmark.

## Reproduction and remaining gates

From the encoder-lab worktree, with existing reference clips and reference tools:

```sh
python3 tools/tvc_ref/enc/profile_phases.py --verify --out docs/encoder_parallel_phase_profile.json
python3 tools/tvc_ref/enc/make_experiment.py bwide
python3 tools/tvc_ref/enc/check_wide.py
python3 tools/tvc_ref/enc/make_experiment.py bslice
python3 tools/tvc_ref/enc/make_experiment.py blook
python3 tools/tvc_ref/enc/make_experiment.py bmotion
ffmpeg -v error -y -stream_loop 3 -i work/encoder_lab/film-input.mkv -t 40 -map 0:v -map 0:a -c copy work/encoder_lab/film-400-repeated40.mkv
python3 tools/tvc_ref/enc/bench_pipeline.py
python3 tools/tvc_ref/enc/bench_overlap.py
```

`bench_pipeline.py` requires the separately built `bsafe` encoder and reference
`tac`/`tnv3` tools. `speed_bench.py --encoder-args='-S 3'` or `'-b 4'` passes
the same options to both binaries. The first attempted pipeline benchmark
accidentally overlapped another validation encode; it was interrupted, remaining
processes were checked, and all reported repetitions were restarted cleanly.

Next candidates for production engineering are parallelizing **already-required**
slices and choosing picture-versus-segment workers under a shared budget. The
current generated prototypes lack complete allocation-failure handling, portable
native Windows/Linux tests, broad race tests, long-film/scene diversity and
system-wide CPU accounting. Do not merge them into production or change B count
solely on these study numbers.

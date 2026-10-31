# E-023–E-026: profiling, motion proposals, B workers and compressed ingestion

2026-10-30. Host experiments on Apple M4 Pro, macOS 26.6.2, based on
production commit `1dde42c1` (E-022). All four evaluations are complete.
Production encoder C, decoder, bitstream, RTL, firmware and film defaults are
unchanged. Generated research encoders live under `work/encoder_lab/`.

## Results and decisions

| Evaluation | Measured result | Decision |
|---|---|---|
| Current CPU profile | RDOQ consumes 37–54% of instrumented runtime; fractional search SAD about 7% | Keep exact RDOQ work on the CPU optimization list |
| VideoToolbox motion seeds | Corpus mean BD-rate −1.49%, versus accepted −1.52%; encoder 4–7% slower | Retain as research, keep disabled |
| Two B-picture workers | 1.29–1.44x whole-encoder throughput, identical streams | Strong next production candidate; explicit context and worker budgeting remain gates |
| VideoToolbox H.264/HEVC ingestion | Identical decoded pixels and film files; no end-to-end speed gain | Keep software ingestion default |

Timing comparisons use three alternating repetitions and median elapsed times.
These are host measurements, not Pocket, Quartus, power or thermal evidence.
The B prototype uses more CPU cores; a throughput gain does not establish better
energy use or per-core efficiency. Trials were run without competing timed jobs.

## 1. Profile the accepted encoder

`encoder_profile_next.json` records inclusive instrumentation, source SHA and
call counts. At QP30, 96 pictures:

| Clip | Instrumented total | RDOQ | Fractional prediction/SAD | FDCT8 |
|---|---:|---:|---:|---:|
| action400 | 1.933 s | 0.952 s (49%) | 0.135 s | 0.091 s |
| anime_dialog400 | 2.498 s | 1.338 s (54%) | 0.179 s | 0.111 s |
| action800 | 5.142 s | 1.897 s (37%) | 0.387 s | 0.343 s |

Timer overhead/compiler perturbation is included. FDCT8 excludes 4x4 transforms;
these counters do not partition every encoder stage. This follows E-022's exact
prefix-count improvement and supersedes the earlier profile for prioritization.
No uninstrumented speed gain is claimed from profiling itself.

## 2. Real VideoToolbox motion proposals

`vt_motion_bench.m` exercises the motion-estimation API with known translated
NV12 images. Returned format is `2C0h`: two Float16 components on a 16-pixel
block grid. Axial translations empirically establish pixel units and a vector
from current position to reference position: a source shift +4 gives vector −4.
Median request/callback latency is roughly 0.4–0.54 ms after warmup, excluding
input preparation. This does not establish the physical GPU/ANE placement.

Preserve the failed calibration: the diagonal (+4,−2) test does not produce the
expected (−4,+2) median reliably. Raw texture can give the wrong horizontal sign;
smoothed texture plus multipass gives (−4,+4). Axial success does not establish
half-pixel precision or reliable general motion. All raw and smoothed runs are
retained in `encoder_vt_motion*.json`.

The optional `vt_seeds.m` bridge prepares actual reconstructed reference/current
luma surfaces, waits for completion, reads finite block vectors and converts to
the encoder's half/quarter-pixel units. The generated variant adds one proposal
to the existing five starting candidates. The original CPU distortion score,
search, refinement and RDO still decide the result. API failures use the original
candidate list. No model was trained and no GPU/ANE speedup is claimed.

All 156/158 requests per measured encode succeeded; retained stderr diagnostics
show about 134–145 ms of bridge work per 96 pictures, including preparation.
`encoder_speed_vt_seeds.json` records action400 53.92→50.40 fps,
anime400 41.59→38.56 fps and bw400 32.56→31.29 fps. Outputs were deterministic
across repetitions, but differ from the accepted encoder, as expected for new
search seeds. The full corpus gate passes against the frozen baseline:
dialog −0.95%, action −1.50%, anime −2.37%, bw −1.14%, mean −1.49% BD-rate.
Mean bins/B 9.679 and bins/MB 51.90 are slightly above accepted 9.676/51.87.
Thus it is slower and does not improve the accepted quality/rate result. No
film acceptance or default enablement is warranted. Evidence:
`encoder_vt_seeds_gate.json`. Hardware-access probes require a native process;
sandbox session creation returned −17691 and is not hardware evidence.

## 3. Isolated B-picture worker prototype

`make_experiment.py bthreads` generates an optional POSIX/pthread encoder.
Mutable picture state, SRC/CUR/reference pointers and coefficient scratch become
thread-local. Each job owns its MB array, reconstruction plane and AQ offsets.
Shared anchor pixels and initialized transform/scan/cost tables are read-only.
At most two B jobs run together, and both are joined before an anchor is reused.
Only the parent writes the stream and updates picture/rate history, in original
decoding order. If a speculative base-QP picture exceeds its ordered allowance,
the parent invokes the original serial QP-retry path. This preserves budgeting
despite speculative work. Short groups use the original serial path.

| Whole encoder case | Serial fps | B-worker fps | Speed ratio |
|---|---:|---:|---:|
| action400 QP30 | 54.02 | 76.10 | 1.409x |
| anime400 QP24 | 22.67 | 29.25 | 1.290x |
| bw400 QP36 | 59.11 | 78.84 | 1.334x |
| action800 QP30 | 21.03 | 30.34 | 1.443x |

Every paired stream hash is identical. See `encoder_speed_bthreads.json` and
`encoder_speed_bthreads_800.json`. The corpus gate preserves every accepted
size/PSNR/bin result (`encoder_bthreads_gate.json`). Five stress cases compare
complete streams and reconstruction to serial, then compare reconstruction with
the frozen decoder. They cover level1 QP retries (42, 28 and 3 raised pictures),
level3 two-slice output, AQ, explicit three slices, disabled partition/quarter-pel
and serial fallback. All pass (`encoder_bthreads_stress.json`). ThreadSanitizer
passes one action400 QP30 24-picture encode with decoded reconstruction match;
log: `work/encoder_lab/bthreads-tsan.log`. This is bounded race evidence, not a
proof for every configuration.

The existing film path also passes on all 240 action study pictures plus silence,
three segments/two processes, level2 800x334. Complete TNV3 file equals accepted
E-022 output: 976528 bytes, max picture 25055 bytes, worst second 160475 bytes,
no QP raises/joint retries. Report: `encoder_film_bthreads.json`. Its rounded
single-run time is validation evidence, not an additional speed benchmark.

Do not promote the generated prototype directly: it depends on source insertion
points and POSIX threads, aborts on some allocation/thread failures, and lacks a
combined process/thread worker budget. The next implementation should use an
explicit encoder context, bounded reusable workers and a serial failure path,
then rerun these gates. Native Windows/Linux builds/runtime, oversubscription
with film process jobs, memory/long-film behavior and more race tests remain
unverified. It has not been combined with VideoToolbox seeds. This is an
encoder-only parallelism experiment; decoder dependencies stay frozen.

## 4. Compressed H.264/HEVC ingestion

`bench_ingest.py` compares FFmpeg software decode with explicit VideoToolbox
surfaces (`-hwaccel_output_format videotoolbox_vld`) and
`hwdownload,format=nv12` before the same fps/scale/planar conversion. Requiring
hardware surfaces avoids silently timing FFmpeg software frames as this path;
physical engine attribution/internal VideoToolbox fallback is not separately
instrumented. All source hashes and actual commands are retained in JSON.

Inputs are 240-picture, 10-second, 8-bit H.264/HEVC derivatives of the action800
study clip. The 1920x802 inputs are upscaled from 800x334, not authentic 1080p/4K
feature films. Silence is included for full film-path tests. Target raw pixels
are 800x334 yuv420p, 24 fps. Each complete decoded pixel stream is byte-identical
between backends for these inputs, including frame order/range consequences.

| Decode + download/convert case | Software median | VT median |
|---|---:|---:|
| H.264 800 | 0.127 s | 0.325 s |
| HEVC 800 | 0.116 s | 0.263 s |
| H.264 1920 | 0.188 s | 0.628 s |
| HEVC 1920 | 0.211 s | 0.465 s |

Full film runs use the unchanged serial TVC encoder, two segment processes,
96-picture segments, TAC audio and TNV3 packaging/checking. A temporary wrapper
injects the hardware options only into the video-input FFmpeg command; production
film source is unchanged. H.2641920: software 8.636 s, VT 8.668 s (0.996x).
HEVC1920: software 8.290 s, VT 8.649 s (0.958x). All complete film files match
byte-for-byte between backends for each source and pass TNV3 validation.
Evidence: `encoder_ingest_decode.json`, `encoder_ingest_film.json`.

Keep software default. VideoToolbox may behave differently on other sources or
systems, but these results do not support enabling it. Full-length films,
native-resolution high-detail inputs, 10-bit/HDR, 4K, other hardware and energy
use remain untested. VideoToolbox encoding of H.264/HEVC cannot substitute for
the required frozen TVC output format.

## Reproduction and handoff

Run from the encoder-lab worktree after the existing corpus/reference setup:

```sh
python3 tools/tvc_ref/enc/profile_hotspots.py --out docs/encoder_profile_next.json
clang -O2 -fblocks -fobjc-arc -framework Foundation -framework VideoToolbox -framework CoreVideo -framework CoreMedia -o work/encoder_lab/vt_motion_bench tools/tvc_ref/enc/vt_motion_bench.m
work/encoder_lab/vt_motion_bench 400 176 0 0 > docs/encoder_vt_motion_400.json
work/encoder_lab/vt_motion_bench 800 336 1 1 > docs/encoder_vt_motion_smooth_800.json
python3 tools/tvc_ref/enc/make_experiment.py bthreads
TVC_ENCODER_CFLAGS=-O3 TVC_ENCODER_BINARY=work/encoder_lab/encoder-bthreads TVC_GATE_OUTPUT=docs/encoder_bthreads_gate.json python3 tools/encoder_lab.py gate
python3 tools/tvc_ref/enc/stress_bthreads.py
python3 tools/tvc_ref/enc/speed_bench.py tools/tvc_ref/tvc_enc work/encoder_lab/encoder-bthreads --cases action:30 anime_dialog:24 bw_room:36 --exact --out docs/encoder_speed_bthreads.json
python3 tools/tvc_ref/enc/make_experiment.py vt
TVC_ENCODER_CFLAGS=-O3 TVC_ENCODER_BINARY=work/encoder_lab/encoder-vt TVC_GATE_OUTPUT=docs/encoder_vt_seeds_gate.json python3 tools/encoder_lab.py gate
python3 tools/tvc_ref/enc/bench_ingest.py --prepare --phase decode --out docs/encoder_ingest_decode.json
python3 tools/tvc_ref/enc/bench_ingest.py --phase film --out docs/encoder_ingest_film.json
```

VT commands require native hardware access. `--prepare` regenerates compressed
fixtures; container bytes/source hashes may differ with tool versions. Film
tests require the existing `work/encoder_lab/tac` and `tnv3` reference binaries.
Manifests `encoder_experiment_{bthreads,vt}.json` retain generated source/binary
hashes and build commands. External binaries are prebuilt: the corpus helper
still builds production and decoder tools, and records which encoder it runs.
Gate outputs now default to separate persistent per-encoder paths. An initial
shared temporary gate file was overwritten by the next experiment; the B gate
was rerun into its named durable output rather than using that lost artifact.

Final unchanged production O3 gate passes (`encoder_study_production_gate.json`):
mean BD-rate −1.52%, bins/B 9.676 versus frozen 9.618, bins/MB 51.87 versus
52.13; every clip has lower interpolated equal-quality bin cost. Frozen decoder
vectors and reconstruction checks pass. No new FPGA fit/timing or Pocket claim.
No active encode/build job, push, SD or VM operation remains at handoff.

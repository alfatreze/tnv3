# Encoder lab execution log

## E-002 — Measure baseline parser work (2026-10-30)

**Change.** Added an instrumented build of the unchanged reference decoder at
`tools/tvc_ref/enc/bin_count.c` and connected it to `tools/encoder_lab.py`.
It counts every context and bypass syntax bin decoded from the generated
streams. The result JSON and run/comparison output now include bins per byte
and bins per macroblock. Added `TVC_ENCODER_ARGS` so a command-line encoder
experiment can be reproduced through the same corpus runner. Baseline corpus
data in `docs/encoder_baseline.json` now includes these measurements; sizes
and PSNR values are unchanged from the frozen baseline.

**Baseline measurements.** Across the 12 clip/QP points, the arithmetic mean
is 9.618 syntax bins/byte and 52.13 syntax bins/macroblock. Individual points
range from 8.9629 to 10.1944 bins/byte and 11.57 to 121.48 bins/macroblock.
This is measured on the four 400x clips, first 96 frames, QP 24/30/36. The
count is syntax bin calls in the unchanged C decoder; it excludes range-coder
initialization and output flush work. It is a software count, not a hardware
clock measurement.

**Verification.** `python3 tools/encoder_lab.py gate` completed with the path
guard, frozen decoder vectors, parse-budget model and all 12 stream/recon
checks passing. It reproduced baseline sizes and PSNR exactly and reported
0.00% BD-rate. Each encoder reconstruction matched the decoder output. The
encoder defaults enable quarter-sample motion, 4x4 luma transforms, scene-cut
I pictures (threshold 0.7), look-ahead QP propagation (strength 2.0), and
RDOQ. Variance AQ is off by default. These features are already in use, despite
the stale introductory encoder comment. Hardware decoder cost is not measured
by this gate; syntax and decoder were unchanged, so cost is expected to remain
the same, but this is not a hardware verification. The full film path and
level-enforced `tnv3 check` were not run.

**Outcome / gate risk.** Baseline parser telemetry is established. During the
next experiment, the gate printed **GATE PASSED** although its own rate results
were +0.61% mean and +3.78% on `anime_dialog`, which violates the stated
acceptance criteria. The comparison currently checks stream validity but does
not fail on non-negative mean BD-rate or a clip above +1%. This is a material
gate defect and the escalation condition in the task instructions; optimization
work stopped here. The gate must be corrected and re-run before another encoder
change is evaluated.

## E-003 — Scene-cut threshold experiment rejected (2026-10-30)

**Change tested.** Changed only the scene-cut threshold from the default 0.7 to
0.9 using `TVC_ENCODER_ARGS='-s 0.9'`. No encoder source or syntax was changed.
The retained result is `work/encoder_lab/scene-cut-0.9.json`.

**Measured result.** BD-rate against the measured baseline: dialog +0.00%,
action -1.33%, anime_dialog +3.78%, bw_room +0.00%, corpus mean +0.61%.
Mean bins/byte was 9.634 versus baseline 9.618; mean bins/macroblock was 52.01
versus 52.13. All 12 streams decoded and all encoder reconstructions matched.
The setting fails the rate criteria and is not accepted. `gate` nevertheless
returned PASSED because of the defect recorded in E-002.

**Verification and remaining gate.** Host-side corpus run only. No full-film,
level-enforcement, Quartus, RTL timing, or Pocket verification. The syntax is
unchanged, but no hardware cost comparison was made. No commit was made. Stop
for owner review of the gate mismatch; after the gate is fixed, scene-cut
thresholds are not a promising next experiment. A measured candidate should
focus on coefficient rate-distortion decisions under unchanged draft-5 syntax,
then pass the corrected corpus gate and the full-film level check.

## E-004 — Gate acceptance repaired (2026-10-30)

Owner authorized continuation after E-002's gate mismatch. The comparison now
requires finite BD-rate, negative corpus mean, and no clip above +1%. It also
requires complete corpus metadata and measured bins at every point, rejecting
any same-QP bin-count rise. This conservative cost condition compares equal
frame counts and fixed clip dimensions, so bins and bins/MB have the same
relative change. This is a host parser-work proxy, not a hardware timing test.
The retained E-003 result now correctly returns failure. No hardware hot path
or cold path changed. Frozen source files remain untouched.

## E-005 — Repeated coefficient lowering rejected (2026-10-30)

Tried repeated rather than single decrement in RDOQ. Full gate ran, with frozen
vectors and all recon checks passing, but rate/cost acceptance failed: dialog
0%, action 0%, anime_dialog 0%, bw_room +0.03%, mean +0.01%. The bw_room QP36
bin count also rose. Reverted the encoder change. Host evidence:
work/encoder_lab/rdoq-repeat.json and rdoq-repeat-gate.log. No hardware path
changed; no Quartus or Pocket measurement. Next: test residual coding using
the motion-vector predictor as an additional inter candidate.

## E-006 — Compare parser work at equal quality (2026-10-30)

Corrects E-004's provisional same-QP cost gate. The mission compares stream
size at equal quality; QP alone does not hold quality fixed when decisions
change. The gate now applies the same log-rate/PSNR integration to syntax bin
counts and requires non-positive equal-quality bin change on EVERY clip.
Clip geometry and frame counts are fixed, so this also compares bins/MB.
The 3.2 clocks/bin + 35 clocks/MB model predicts no parser-cost rise when bins
fall at fixed geometry; neither measured hardware clocks nor full reconstruction
engine cost is claimed. Actual same-QP bins/byte and bins/MB remain reported.
Alternative: reject every same-QP increase; rejected because this confounds
quality improvements with cost regressions. This uses three-point quadratic
interpolation, like the existing BD-rate metric; it is an estimate between
measured samples, not additional measured operating points.
Host control checks accepted synthetic -10% byte/bin curves and rejected
unchanged/+10% curves; the retained E-003 failure also remains rejected.
No hardware hot/cold path changes or FPGA resource/timing changes.

## E-007 — Predictor-vector residual candidate accepted (2026-10-30)

**Decision.** For each inter list combination, also evaluate residual coding
using the predicted vector(s) when they differ from the SAD-search winner.
Previously predicted vectors were considered only as SKIP, without residuals.
The existing complete macroblock rate-distortion cost selects the candidate.
One additional candidate per list combination; no syntax, flags, decoder, RTL
or firmware changes. Encoder analysis hot path gains work; hardware hot/cold
paths are unchanged. Rejected E-005's coefficient approach is retained above.

**Host result.** Full corrected gate PASSED, all 12 encoder reconstructions
match the frozen decoder, frozen vectors and parse-budget model pass, guard
passes. Evidence: docs/encoder_predictor_residual.json and
work/encoder_lab/predictor-residual-final-gate.log. Per-clip BD-rate and
equal-quality interpolated bin changes, respectively:

| clip | BD-rate | bin change at equal PSNR | mean bins/B baseline -> new |
|---|---:|---:|---:|
| dialog | -0.92% | -0.31% | 9.877 -> 9.928 |
| action | -1.58% | -0.74% | 9.365 -> 9.448 |
| anime_dialog | -2.45% | -1.88% | 9.409 -> 9.464 |
| bw_room | -1.13% | -0.74% | 9.821 -> 9.865 |

Mean BD-rate is -1.52%; measured arithmetic mean bins/B is 9.676 versus 9.618
(+0.058), and bins/MB is 51.87 versus 52.13. Bins/B alone rises as the encoder
compresses the syntax better; equal-quality bin counts decrease on each clip.
See E-006 for the interpolation/cost interpretation and limits.

**Film-path host integration.** Passed encode_film.py then explicit `tnv3 check`
on all 240 frames of action_400 (10 s), 400x166 at 24 fps, QP30, level1,
three 96-frame segments, two jobs. Source was losslessly wrapped with FFV1
and silent 48 kHz stereo PCM using ffmpeg (shortest input). Work artifacts:
film-input.mkv, film-result.tnv3, film-result.json, film-path.log under
work/encoder_lab/. encode_film.main() was invoked with its TAC and TNV3 module
constants set to work/encoder_lab/tac and work/encoder_lab/tnv3, compiled with
`cc -O2` from unchanged reference sources; other defaults stayed intact.
Result: valid TNV3 with CRC and audio; 447692 total bytes, max picture 11213 B,
worst second 70846 B, no level QP raises or joint re-encodes. This exercises
the full container pipeline and level enforcement on a real-content study
clip, not a full-length feature film, high levels, or a stressed rate limit.
No quality claim is derived from ffmpeg's PSNR/VMAF path.

**Remaining limits / handoff.** No active builds after these checks. No Quartus,
RTL simulation, Pocket measurement, panel test, full-length-film run, or
measured hardware timing/resource comparison; no fit touched, figures pending.
Next most promising experiment: rate-distortion refinement near the searched
motion vector, bounded to a few candidates, with the same corpus and film gates.
No push or SD-card operation performed.

## E-008 — Copy only coefficient-class contexts (2026-10-30)

Encoder analysis hot path: coefficient trial counting now copies only the
last/sig/gt1/lrem arrays for the selected class. All are the same live models
as before; unused class/models are never read by code_block_n (code-review).
Full gate PASSED; corpus bytes, bins and PSNR identical to E-007. Three
alternating before/after repetitions, 96 frames, -O2, no concurrent gate:
median action QP30 3.1434 -> 3.0527 s (1.0297x), anime_dialog QP24
6.5225 -> 6.3912 s (1.0205x), bw_room QP36 3.4053 -> 3.3650 s (1.0120x).
All paired stream SHA256 hashes identical. Host evidence:
docs/encoder_speed_context.json; work/encoder_lab/context-copy-gate.log and
bench_speed.py. Small host timing differences may include system noise; no
hardware timing/resource change claimed. Decoder and hardware paths untouched.
Accepted as an exact small speed improvement. Next gate: integer-SAD fast path.

## E-009 — Direct in-bounds integer motion SAD (2026-10-30)

Encoder motion-search hot path now measures SAD directly from source/reference
pixels for integer vectors whose complete 16x16 block is in bounds. Fractional
and border vectors retain the original prediction/clamping path. This removes
a temporary prediction block and per-pixel clamping for the common integer
case; arithmetic and search decisions stay identical.
Host: full gate PASSED with exact E-007 corpus sizes, PSNR and bins.
20,000 differential cases against the frozen interpolation functions, integer
and fractional half/qpel vectors, negative displacements and borders, passed
with address/undefined-behaviour sanitizers. Reproduce using
`cc -O2 -fsanitize=address,undefined -o work/encoder_lab/test_sad tools/tvc_ref/enc/check_sad.c -lm`
then run that binary.
Three alternating timing repetitions vs pre-speed E-007 executable (both -O2),
96 frames: action QP30 3.1146 -> 2.5662 s, 37.41 fps, 1.2137x; anime_dialog
QP24 6.4618 -> 5.7842 s, 16.60 fps, 1.1172x; bw_room QP36 3.3739 ->
2.6137 s, 36.73 fps, 1.2908x. Every benchmark stream SHA256 is identical.
Evidence: docs/encoder_speed_integer_sad.json; work/encoder_lab/integer-sad-gate.log.
Reusable timing tool: tools/tvc_ref/enc/speed_bench.py, --exact, with alternating
order and binary/stream hashes. Timing is host-specific and not a guarantee
on other machines or clips. No hardware hot/cold path, fit or timing changed.
Accepted. Next: avoid recomputing the accepted 8x8 luma inverse transform.

## E-010 — Reuse luma inverse transform (2026-10-30)

Encoder residual-analysis hot path now returns the accepted inverse transform
from code_residual instead of recomputing it in code_luma_residual. Zero/
rejected blocks keep the caller-zeroed residual. Chroma does not request it.
Full gate PASSED with exact E-007 corpus metrics. Three alternating timing
repetitions vs E-009, 96 frames, -O2, identical paired stream hashes:
action:30: 2.5901 -> 2.5303 s (1.0236x).
anime_dialog:24: 5.7827 -> 5.7317 s (1.0089x).
bw_room:36: 2.6303 -> 2.5878 s (1.0164x).
Evidence: docs/encoder_speed_reuse.json; work/encoder_lab/reuse-transform-gate.log.
Small 0.9–2.4% host speed gains, subject to timing noise; accepted because it
removes redundant work and preserves streams. No decoder/hardware path or fit
change. Next gate: avoid retaining unused reconstructions without -r.

## E-011 — Retain reconstructions only for -r (2026-10-30)

Encoder memory/copy hot path: ordinary encodes no longer allocate and retain
all output reconstructions. The three anchor buffers remain unchanged and
one B-picture buffer is reused. With -r, all frame reconstructions remain
available for display-order output as before. This reduces per-segment memory
for the existing parallel film workflow without changing decisions or syntax.
The whole source input and look-ahead are still held in memory; bounded-memory
streaming is deferred. Corrected the stale encoder introduction to describe
existing scene-cut, qpel, 4x4 and RDOQ features accurately.

Host full gate PASSED with TVC_CHECK_NO_RECON=1: all 12 corpus streams match
with and without -r, and all reconstructions match the frozen decoder. Corpus
bytes, PSNR and bins remain exact E-007 values. Evidence:
docs/encoder_fast.json and work/encoder_lab/memory-gate.log. The optional
no-reconstruction equivalence check is reproducible through the gate using
that environment variable.

On all 240 frames of action_800, QP30, no -r, E-010 versus new: peak RSS
225460224 -> 121602048 bytes (215.02 -> 115.97 MiB), a 46.06% reduction.
Elapsed 17.697 -> 17.399 s (one pair only; not a repeated timing claim).
Streams have identical SHA256. Evidence: docs/encoder_memory.json and
work/encoder_lab/measure_memory.py. macOS wait4 ru_maxrss reports bytes.
The optional /usr/bin/time -l probe could not read kern.clockrate in the
sandbox, so direct per-child os.wait4 resource usage was used instead.
Neither hardware hot/cold path nor FPGA fit/timing changed; hardware resource
figures remain unmeasured. Accepted; next: final cumulative timing and film check.

## E-012 — Final speed/quality stopping point (2026-10-30)

Final repeated benchmark vs E-007 before speed work, both -O2, no competing
build/encode, three alternating repetitions per case. All paired stream hashes
identical. Evidence: docs/encoder_speed_final.json; reproduce using
`python3 tools/tvc_ref/enc/speed_bench.py work/encoder_lab/encoder-pre-speed tools/tvc_ref/tvc_enc --exact --out work/encoder_lab/speed-final.json`.
The benchmark validates requested frame counts before reporting fps.

| case | old median s | new median s | final fps | throughput ratio |
|---|---:|---:|---:|---:|
| action:30 | 3.1092 | 2.5511 | 37.63 | 1.2188x |
| anime_dialog:24 | 6.4444 | 5.7390 | 16.73 | 1.1229x |
| bw_room:36 | 3.3844 | 2.5867 | 37.11 | 1.3084x |

Film integration rerun on the complete 240-frame 400x166 action study clip
with silent audio, level1, three segments, two jobs, QP30. encode_film.py and
explicit `work/encoder_lab/tnv3 check work/encoder_lab/film-fast.tnv3` pass;
`cmp work/encoder_lab/film-result.tnv3 work/encoder_lab/film-fast.tnv3` passes
against E-007. Final full TNV3 output is byte-identical. The report rounds full
pipeline time to 8 s for 10 s of content. Evidence: docs/encoder_film_fast.json
and work/encoder_lab/film-fast.log. Note encode_film.main calls sys.exit, so
explicit checker and cmp were run separately after the pipeline command.

Quality/cost result remains E-007: BD-rate dialog -0.92%, action -1.58%,
anime_dialog -2.45%, bw_room -1.13%, mean -1.52%; measured average bins/B
9.676 versus frozen baseline 9.618, bins/MB 51.87 versus 52.13. Equal-quality
interpolated bin changes are -0.31%, -0.74%, -1.88%, -0.74%, respectively.
All subsequent speed changes preserve streams, rather than spending quality
for speed. No new decoder syntax or hardware-path changes.

Assessment: a practical high-quality reference-encoder stopping point for
this host, with 12–31% higher measured throughput and 46% lower no-recon RSS.
The 800x334 no-recon test processes 240 frames in 17.40 s (13.79 fps), so
single-process real-time throughput is NOT established at 800 pixels. Use
encode_film.py's existing segmented parallel path for throughput; memory still
scales with source frames and worker count. Encoder remains single-threaded.
No subjective quality study, full-length feature film, stressed rate-limit
case, level2/3 film validation, RTL simulation, Quartus fit, hardware timing or
Pocket verification was performed here. Current quality evidence is PSNR-Y
and unchanged streams; no trusted VMAF claim. Hardware resource/timing figures
remain unmeasured, with source frozen.

No active builds or experiments remain. The working tree is intended clean
after this commit. No push or SD-card operations. Next promising improvement
is exact reuse of coefficient-count prefix state during RDOQ, but it needs
careful differential tests for last-coefficient changes and is deferred;
further heuristic quality/speed trade-offs are unnecessary for this stage.

## E-013 — Portable pixels and optional CPU SIMD (2026-10-30)

Encoder motion-analysis hot path now uses a single encoder-only 16x16 SAD
kernel with C, ARM64 NEON and x86 SSE2 implementations. The scalar C source
can already auto-vectorize. Intrinsics use exact integer accumulation, no
out-of-block reads, support odd strides and unaligned addresses. CPU target,
not OS, chooses the instruction family. Native MSVC ARM64 header selection
is supplied from Microsoft documentation but is uncompiled here.

Native NEON versus E-011, three alternating repetitions: action QP30 1.0193x,
anime_dialog QP24 0.9936x, bw_room QP36 1.0080x. Identical streams. These
changes are too small/mixed to establish a reliable overall benefit: explicit
SIMD was rejected as the default and retained behind TVC_SIMD_PIXELS.
TVC_PORTABLE_PIXELS overrides the opt-in. No -march=native, AVX or fast-math
requirement was added. Decoder/hardware hot and cold paths are unchanged.

Host evidence: docs/encoder_speed_simd.json, encoder_simd_native.json (native
full gate before the opt-in decision, when NEON was auto-selected), and
encoder_portable_pixels.json (final default C full gate). Both pass the full
gate with E-007 metrics. Explicit SSE2 x86-64 full gate also PASSED under
Rosetta: docs/encoder_sse2_rosetta.json. Its mean BD-rate is -1.59% versus
the ARM-generated frozen corpus baseline; existing floating analysis differs
across targets, so this is not attributed to SSE2. All reconstructions match
the unchanged ARM reference decoder. Paired x86 C/SSE2 streams match exactly
on three sample cases: encoder_sse2_equivalence.json; its one-run emulated
timings with a concurrent gate are NOT native performance evidence.

20000 differential motion cases plus zero/full-contrast tests at every byte
alignment and odd strides pass on native NEON, C, and emulated SSE2. Native
NEON tests include address/undefined-behaviour sanitizers. No Linux runtime,
Windows runtime/toolchain, native Intel/AMD timing, or hardware decoder tests
were performed. No FPGA fit/resource/timing changes.

TVC_ENCODER_CFLAGS now passes investigation flags only to the encoder and
records them in result JSON; decoder/counter flags remain frozen at -O2.
Build/path decisions, commands, OS/ISA matrix, external primary references
and pending platform gates are in docs/ENCODER_CPU.md. OS-specific floating
transform libraries and hardware video backends are deferred, with no TVC
compatibility or speed gain established. Next experiment: replace motion
cost log2 with exact integer bit scan and compiler/C fallbacks.

## E-014 — Exact integer motion-vector costs (2026-10-30)

Encoder motion-analysis hot path replaces floor(log2(abs(d))) with an integer
highest-set-bit calculation. GCC/Clang use guarded __builtin_clz, MSVC uses
_BitScanReverse, and TVC_PORTABLE_BITS/other compilers use a C shift loop.
Unsigned magnitude conversion also handles INT_MIN without signed overflow.
Zero is handled before intrinsics. No optional ISA requirement, fast math,
decoder or hardware path changes. Generated test assembly contains baseline
ARM64 clz and x86 bsrl. Compiler selection/zero semantics are documented with
primary references in ENCODER_CPU.md.

Host full gate PASSED, exact E-007 corpus metrics: docs/encoder_bitops.json.
Native sanitized, portable C and Rosetta x86 checks match floating reference
on all -65535..65535 inputs and integer extrema/power boundaries. Three
alternating same-host benchmark repetitions vs E-013, exact stream hashes:
action:30: 2.6303 -> 2.5168 s, 1.0451x.
anime_dialog:24: 5.6979 -> 5.7309 s, 0.9942x.
bw_room:36: 2.5942 -> 2.5796 s, 1.0056x.
Evidence: encoder_speed_bitops.json and work/encoder_lab/bitops-gate.log.
Action improves 4.5%; other changes are within +/-0.6% timing noise. Accepted
as exact removal of unnecessary libm work, without claiming a large universal
throughput gain. MSVC and native Linux/Windows builds/runtimes remain pending;
no FPGA resource or timing measurement. Next: test generic -O3 code generation
without fast math before choosing a build profile.

## E-015 — Measured portable compiler fast profile (2026-10-30)

Evaluated -O3 appended to the encoder's -O2 build command, without fast math
or host-specific target flags. The default build remains -O2 for established
comparisons; -O3 is the explicitly selected fast profile. Frozen decoder and
counter builds remain -O2. Full native ARM64 gate PASSED with unchanged E-007
corpus metrics, encoder/decoder reconstruction equality and parser cost.
Evidence: docs/encoder_o3.json and work/encoder_lab/o3-gate.log.

Three alternating O2/O3 repetitions, 96 frames each, all paired streams exact:
action:30: 2.5644 -> 2.4421 s; 39.31 fps, 1.0501x.
anime_dialog:24: 5.7985 -> 5.5619 s; 17.26 fps, 1.0425x.
bw_room:36: 2.6120 -> 2.4034 s; 39.94 fps, 1.0868x.
This 4.3–8.7% throughput improvement is more consistent than the explicit
NEON experiment. Evidence: docs/encoder_speed_o3.json. Encoder analysis hot
path code generation changes; codec syntax, decoder/hardware hot/cold paths
and their build flags do not. No FPGA fit/resource or hardware timing claim.
Full-encoder assembly inspection confirms ARM64 clz and auto-vectorized NEON
uabd in the default C path, and psadbw in the opt-in x86 SSE2 build. Assembly
evidence is work/encoder_lab/cpu-encoder-arm64.s and cpu-encoder-x86.s.

Full film path rerun with the O3 binary, original complete 240-frame action
study source plus silence, level1, three segments and two jobs. Pipeline and
explicit tnv3 check pass; cmp against the E-007 TNV3 output passes. The report
rounds total time to 7 s for 10 s source; this is one integration run, not a
repeated timing result. Evidence: docs/encoder_film_cpu.json and
work/encoder_lab/film-cpu.log. No full-length film, stressed level limit,
level2/3 pipeline or subjective quality verification.

Current built encoder is the -O3 native ARM64 fast profile. Reproduce with
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py gate (or build). A plain
gate/build restores -O2; this choice is intentional and recorded in CPU.md.
Default C pixel source remains auto-vectorizable; explicit SIMD stays opt-in.
Native macOS ARM64 and x86-64 via Rosetta are tested. Windows/MSVC, Linux/GCC,
native Intel/AMD performance and Windows ARM64 paths remain external gates.
No new OS-specific codec backend, external library, runtime feature probing
or unsafe machine-native build flags were introduced.

Handoff: no active build/benchmark remains; no push or SD operation. Mean
BD-rate remains -1.52% (dialog -0.92%, action -1.58%, anime_dialog -2.45%,
bw_room -1.13%). Mean bins/B 9.676 vs frozen 9.618; bins/MB 51.87 vs 52.13;
equal-quality bin-change estimates remain negative on every clip. The E-011
46% no-recon RSS reduction remains intact. This is a practical continuation
stopping point. Next useful ISA gate is native Windows/Linux Intel/AMD
benchmarking before considering AVX2 defaults; the next larger exact CPU
optimization remains RDOQ prefix-count reuse, deferred pending dedicated
differential coverage. No hardware timing or Pocket evidence was added.

## E-016 — GPU/Neural Engine feasibility measured (2026-10-30)

Decision: retain the CPU production encoder. GPU source-look-ahead alone is
a low-value offload; a future GPU prototype should target frame-batched
encoding-stage candidate work after finer profiling. ANE is deferred until a
learned proposal model and training/evaluation plan are justified. Alternatives
considered: synchronous per-macroblock Metal calls, source-only look-ahead,
floating transform offload, learned motion/cut proposals. Detailed evidence,
trade-offs, primary sources and next gates: docs/ENCODER_ACCELERATORS.md.
Neither production hot nor cold path changed; only optional research tools
and documents were added. No FPGA/decoder interface, syntax or hardware fit
change, and no new Pocket/timing/resource claim.

Host GPU evidence: MTLCreateSystemDefaultDevice returned nil in the sandbox.
Authorized execution outside it detected Apple M4 Pro with unified memory.
metal_probe.m then compiled/executed a one-integer-increment compute kernel,
verified completion and output, and measured 50 warm trials after five warmups.
Median CPU submit/wait wall time 131 us (p95 154 us), GPU median 7 us; initial
pipeline setup 131 ms. Evidence: docs/encoder_gpu_probe.json. This is dispatch
latency, not actual TVC throughput, an asynchronous/batched latency bound or
an energy comparison. No earlier GPU measurement exists for comparison.

Host phase profile (O3, QP30, 96 frames each) measured look-ahead share:
action_400 1.209%, anime_dialog_400 0.799%, bw_room_400 0.787%, action_800
1.364%. Infinite-speed zero-overhead removal gives estimated total speedups
1.01224x, 1.00805x, 1.00793x, 1.01383x. Main encode averages 64.07–88.12
us/MB, so naive per-block synchronous offload is unattractive on this measured
setup. Coarse single profiles, not a fine-grained statistical attribution.
Each instrumented stream was SHA256-equal to the production O3 encoder.
Evidence: docs/encoder_gpu_phases.json; reproducible optional tool
tools/tvc_ref/enc/profile_phases.py --verify. Timers are inserted into a
temporary work/ C copy with unique-pattern checks; original source untouched.

No actual TVC GPU backend, SAD GPU throughput benchmark, ANE model, Core ML
placement test, learned training, Windows/Linux accelerator run, full-length
film offload or power/thermal study was performed. Main encoding still
contains coupled prediction, reconstruction and adaptive-context decisions;
a GPU redesign's speed/quality benefit remains unmeasured. Existing E-015
corpus/full-film gates remain the production evidence: mean BD-rate -1.52%,
per-clip -0.92/-1.58/-2.45/-1.13%, bins/B 9.676 versus 9.618, bins/MB
51.87 versus 52.13, negative interpolated equal-quality bin change per clip.
No new encoder gain is claimed.

No active jobs after these probes. Current encoder remains the native O3 CPU
fast profile; no GPU/ANE backend is enabled or linked. Nothing pushed and no
SD operation. Next gate: profile the main encoding candidate/RDOQ work before
committing to a batched Metal implementation; any implementation then needs
full corpus and level-enforced film validation.

## E-017 — VideoToolbox routes and correction to E-016 (2026-10-30)

New evidence refines E-016: the Apple SDK contains public macOS26 motion-
estimation APIs accepting frame pairs and 4x4/16x16 search blocks. Our own
trained Core ML model is not a prerequisite for obtaining Apple-provided
motion proposals. The previous assessment missed this route; its custom-model
requirement was too broad. The backing accelerator/ANE placement is unverified
and is not inferred from third-party descriptions. Detailed revision is
prepended to docs/ENCODER_ACCELERATORS.md; E-016 remains retained.

Host probe: on macOS26.6.2/M4 Pro, 16x16-block sessions create and return source
attributes at 400x176, 800x336 and 1920x1080, all status0. In the sandbox each
returned -17691 (SDK kVTSessionMalfunctionErr); authorized out-of-sandbox
execution succeeded. H264 and HEVC hardware-decode support queries return
true. Evidence: encoder_videotoolbox_probe.json and
encoder_videotoolbox_sandbox.json; optional probe source
tools/tvc_ref/enc/videotoolbox_probe.m. No frames submitted, vector outputs,
throughput, actual ANE/hardware placement or encoder rate changes measured.

Decision: investigate this API as optional motion-search seeds, preserving
CPU final RDO and the existing bitstream/decoder. Source VideoToolbox decoding
is separately viable for supported compressed inputs; no speedup measured,
and current Y4M clips have no compressed decode stage. Standard hardware
H264/HEVC encoding is not a TVC replacement under the frozen decoder contract.
No production hot/cold paths or hardware fit/timing/resource values change.
Existing E-015 full gate and film validation remain production evidence; no
new compression or acceleration gain claimed. No source codec/RTL/firmware
files changed, no push/SD operation, no active jobs after this probe.
Next gate: translated-frame vector validation and per-frame timing, then the
full corpus/parser-cost/film gates for any optional TVC candidate integration.

## E-018 — dav1d engineering review and adaptation plan (2026-10-30)

Code-review: examined VideoLAN's generic MC, ARM/x86 DSP dispatch, CPU/OS feature
checks, checkasm comparisons/benchmarks, picture allocation/lifetime, task
scheduling, arithmetic decoder and COPYING. Moving master references and
review date are recorded in docs/ENCODER_DAV1D.md; no source imported or
external dependency installed. Future imports must pin a revision and retain
applicable author/license notices; no licensing compatibility ruling made.

Decision: prioritize checkasm-style whole-pixel differential coverage and
encoder-only fractional MC kernels after fine profiling, followed by measured
DSP dispatch and a separately designed worker/context model. Existing SAD/
bit checks, C fallback and ISA opt-in policy already partially follow this
method. Rejected direct AV1 filter/transform/entropy transplant: wrong codec
semantics and frozen decoder contract. Rejected naive task-thread copying:
current encoder state is global, references need explicit lifetimes, and
level-rate budgeting/output must remain ordered. dav1d allocator padding must
not silently alter TVC dimensions/stride/clamp semantics.

No production hot/cold path changed, no benchmark or acceleration gain claimed.
No tests re-run for this documentation-only source review; path guard and
diff checks pass. E-015 remains production evidence (mean BD-rate -1.52%;
per clip -0.92/-1.58/-2.45/-1.13%; bins/B 9.676 versus baseline 9.618).
No FPGA fit/resource/timing, Pocket, Linux/Windows native or new licensing
verification claim. No active build, no push/SD operation. Next gate: fine
profile main encoding, then exact fractional-kernel prototype with pixel
differential, alternating throughput, full corpus/parser-cost and film gates.

## E-019 — Main-stage hotspot profile (2026-10-30)

Host coarse instrumentation on pre-change source 2ba43ad0: fractional search
prediction/SAD ~19–26% and RDOQ ~33–50% of instrumented runtime across
action_400, anime_dialog_400 and action_800, 96 frames QP30. Source hashes,
raw times and call counts in docs/encoder_hotspots.json. Timing-call overhead
and compiler perturbations are included; fdct8 excludes 4x4 transforms.
This is hotspot selection evidence, not a new unbiased throughput claim.
Optional profile_hotspots.py generates a work/ source copy with checked timing
insertion patterns. No production hot/cold path or hardware changed in this
profiling step. Selected fractional kernels as an exact first implementation;
RDOQ remains a next candidate after re-profiling.

## E-020 — Exact interior quarter-pel search kernels accepted (2026-10-30)

Encoder search hot path now uses private interior integer/horizontal/vertical/
2-D phase kernels with frozen TVC coefficients. The 2-D case reuses 19 rows
of unrounded/unclipped horizontal intermediate results. Border footprints
retain the unchanged reference function. No dav1d source copied; this applies
its specialized-kernel/reference-test approach, not AV1 filter semantics.
No decoder/RTL/firmware/shared-header change, bitstream syntax or new flags.
Alternatives, arithmetic bounds, instructions and full evidence are recorded
in docs/ENCODER_QPEL.md. No new per-call heap allocations.

Host tests: 30720 whole-pixel phase/border/pattern/stride/alignment cases,
native ASAN/UBSAN and Rosetta x86 pass, plus existing 20000 SAD cases. Full
O3 and default O2 gates PASSED with exact prior corpus bytes, PSNR and bins. Decoder recon
match, frozen vectors and parser cost checks all pass. Evidence:
docs/encoder_qpel.json and encoder_qpel_o2.json; work/encoder_lab qpel gate logs.
The O3 fast build was restored after the default O2 check.

Three alternating whole-encoder repetitions vs already-fast pre-change O3,
all paired stream hashes identical: action400 QP30 39.04 ->45.59 fps (1.1677x),
anime400 QP24 17.21 ->18.27 (1.0613x), bw_room400 QP36 39.80 ->52.48
(1.3187x), action800 QP30 14.71 ->18.76 (1.2751x). Evidence:
encoder_speed_qpel.json and encoder_speed_qpel_800.json. Host/M4 Pro figures
only; no native Windows/Linux/Intel performance inference.

Full-film-path host integration on 240 action study frames plus silence: level1
400x166 output byte-identical to E-007; level2 800x334 before/after TNV3
byte-identical, both pass tnv3 check. Level2: 976528 bytes, largest picture
25055 bytes, worst second160475 bytes, no level QP raises or joint re-encodes.
Three segments/two workers. Reports: encoder_film_qpel.json,
encoder_film_qpel_800_before.json and encoder_film_qpel_800.json. Study clips
are not full-length films, level3 or stressed rate-cap cases.

Quality/rate result retained: dialog -0.92%, action -1.58%, anime_dialog -2.45%,
bw_room -1.13%, mean -1.52% BD-rate. Mean bins/B remains 9.676 vs9.618
frozen baseline; bins/MB51.87 vs52.13 and equal-quality bin changes negative
on all clips. No new compression gain claimed; this is exact CPU speed work.
No FPGA resource/timing change or Quartus/Pocket evidence; source frozen.
Next: re-profile remaining RDOQ/candidate work, then consider exact prefix-
count reuse; native platform gates remain pending. Current binary is O3.
No active encode/build jobs at handoff; no push or SD operation.

## E-021 — Post-fractional-kernel RDOQ profiling (2026-10-30)

Host coarse instrumentation on 568ecb9a records RDOQ at ~54–58% of
instrumented QP30 runtime on action/anime400 and ~41% on action800; fractional
search prediction/SAD ~6–7%. Timers/compiler perturbation are included and
fdct8 excludes 4x4 transforms, so these are hotspot indicators. Raw times and
source hashes: docs/encoder_hotspots_post_qpel.json. Selected exact prefix
state reuse over another motion-kernel experiment. No production path, decoder
or hardware change in this profiling step; no uninstrumented gain claim.

## E-022 — Exact RDOQ prefix counting accepted (2026-10-30)

Encoder analysis hot path caches coefficient-class contexts, higher-symbol
greater-than-one count and the accumulated RC_CNT floating cost. Each trial
resumes suffix counting in original order. Last-position changes use full
replay and restart the prefix when accepted. Singleton fast paths avoid
unnecessary setup while preserving the old one-decrement rule. Final live
contexts are unchanged. This retains the greedy algorithm and approximate
cost model, with no codec/decoder syntax, transform, coefficient choice or
quantization-model change. Alternatives, invariants and reproduction are in
docs/ENCODER_RDOQ_PREFIX.md. No dav1d source copied; shared tvc.h stays frozen.

Host verification: 12000 synthetic differential blocks versus the pre-change
algorithm, all classes and all QPs, with every cached trial checked against
full replay. Outputs/counts and live-context preservation pass native
ASAN/UBSAN and Rosetta x86 checks. Full O3 and default O2 gates both PASS,
with exact prior corpus size/PSNR/bins, frozen vectors, reconstruction match
and parser-cost acceptance. Evidence: encoder_rdoq_prefix.json,
encoder_rdoq_prefix_o2.json and work/encoder_lab gate logs. Profiling helper
updated for the new wrapper and exercised on all three sample cases.

Three alternating paired O3 whole-encoder repetitions, all stream hashes
identical: action400 QP30 46.90 ->52.11 fps (1.1110x), anime400 QP24
18.81 ->21.59 (1.1475x), bw400 QP36 53.53 ->57.20 (1.0687x), action800
QP30 18.67 ->20.27 (1.0860x). Raw evidence: encoder_speed_rdoq_prefix.json
and encoder_speed_rdoq_prefix_800.json. Host/M4 Pro measurements only.

Full-film-path host integration: all 240 action study frames plus silence,
three segments/two workers, level1 400x166 and level2 800x334. Both pass
tnv3 check and complete-file cmp against earlier accepted TNV3 files.
Level1 output447692 B; level2 976528 B, max picture25055 B, worst second
160475 B, no raised QPs/joint retries. Reports encoder_film_rdoq_prefix.json
and encoder_film_rdoq_prefix_800.json. Not full-length films or stressed
level-cap/level3 tests. No subjective study, native Windows/Linux timing,
new FPGA resource/fit/timing or Pocket evidence; no hardware paths touched.

Rate retained: dialog -0.92%, action -1.58%, anime_dialog -2.45%, bw_room
-1.13%, mean -1.52% BD-rate. Bins/B9.676 vs frozen9.618, bins/MB51.87
vs52.13; equal-quality bin changes remain negative on every clip. No new
compression gain claimed. Current binary restored to O3 after default O2
verification. No active jobs/builds at handoff, no push/SD operations. Next:
re-profile remaining counting/model-copy cost before another exact optimization.


## E-023 — Re-profile accepted E-022 encoder (2026-10-30)

Host instrumentation on production 1dde42c1: RDOQ is 49% action400, 54%
anime400 and 37% action800 at QP30; fractional search/SAD about 7%. Inclusive
timer/compiler overhead included, FDCT8 excludes 4x4. Evidence:
encoder_profile_next.json. Hot-path observation only; no production change or
speed gain. Exact RDOQ work remains a candidate; B-picture concurrency was also
selected for evaluation. No hardware resource/timing changes. Detailed decision,
alternatives and remaining gates: ENCODER_FOUR_STEP_STUDY.md.

## E-024 — VideoToolbox motion proposals rejected as default (2026-10-30)

Host calibration confirms 2C0h Float16 vectors on a 16-pixel block grid and
pixel units/current-to-reference sign for axial shifts. Diagonal (+4,-2)
calibration fails: raw noise sometimes returns wrong sign, smooth multipass
returns (-4,+4), not expected (-4,+2). Keep all evidence in encoder_vt_motion*
JSON; axial success is not subpixel correctness. Warm request/callback median
~0.4–0.54 ms excludes preparation; physical accelerator placement unverified.
Sandbox session -17691 is an access result, not proof of unsupported hardware.

Generated optional VT hot-path seed variant uses real reconstructed anchors,
CPU scoring/refinement and failure fallback, leaving production C unchanged.
Full corpus gate PASS versus frozen baseline, but mean BD-rate -1.49% versus
accepted -1.52%; bins/B9.679, bins/MB51.90 versus accepted9.676/51.87.
Three alternating host speed repetitions are 4–7% slower including preparation
and synchronization. Requests succeed156/158 per sample; hashes deterministic.
Evidence encoder_vt_seeds_gate.json, encoder_speed_vt_seeds.json, generated
build manifest encoder_experiment_vt.json. Rejected default activation and
film acceptance; retain research for future batching rather than infer gain
from API latency. No model/ANE or FPGA/Pocket claim. Next gate is a demonstrable
whole-encoder benefit with robust calibration before reconsidering.

## E-025 — Private B-picture worker prototype evaluated (2026-10-30)

Generated optional pthread/TLS prototype isolates picture, reconstruction and
scratch state, keeps anchors immutable through join, commits rate history and
stream in original order, and serially retries an over-budget speculative
picture. Two B jobs at most; short groups retain serial behavior. This targets
the encoder hot path without decoder/syntax changes. Alternatives: segment-only
parallelism stays default; production explicit context and bounded reusable
workers deferred until integration. No production C change.

Three alternating paired host O3 repetitions: action40054.02->76.10fps1.409x,
anime40022.67->29.25fps1.290x, bw40059.11->78.84fps1.334x,
action80021.03->30.34fps1.443x. All streams exact; corpus gate preserves accepted
-1.52% mean BD-rate and all bins/PSNR/size results. Five stress cases pass exact
serial stream/recon and frozen-decoder comparison, covering level retries
(42,28,3 raised pictures), level3/two slices, AQ, explicit slices and fallback.
Host ThreadSanitizer one24-picture action case passes with reconstruction
match (work/encoder_lab/bthreads-tsan.log); not universal race proof.

240-picture level2 film integration passes TNV3 and complete-file comparison:
976528B, maxpic25055B, worstsecond160475B, no raises/joint retries. Evidence:
encoder_bthreads_gate/stress.json, encoder_speed_bthreads*.json,
encoder_film_bthreads.json, encoder_experiment_bthreads.json. Initial shared
/tmp gate evidence was overwritten by the following experiment; reran B gate
into its named persistent artifact. Harness now selects/records a prebuilt
in-worktree experimental encoder and uses per-encoder persistent gate outputs.

Outcome: strongest next production candidate; remains optional generated
research code. More CPU cores used; no power/per-core efficiency claim. Risks:
POSIX-only, source insertion drift, allocation/thread failure handling,
process/thread oversubscription, native Windows/Linux, long-film memory and
broader race testing. Next gate: explicit context plus global worker budgeting
and serial fallback, then repeat all gates. No hardware resource/fit/timing or
Pocket change/evidence.

## E-026 — Compressed ingestion measured; software retained (2026-10-30)

Host FFmpeg explicit VideoToolbox surfaces plus hwdownload compared with SW
H.264/HEVC decode, identical fps/scale/planar output. Three alternating runs
per backend/source. Entire raw pixel streams match for all four inputs.
Decode medians SW/VT seconds: H264800 .127/.325, HEVC800 .116/.263,
H2641920 .188/.628, HEVC1920 .211/.465. Full film path (two processes, serial
production TVC) H2641920 8.636/8.668s, HEVC1920 8.290/8.649s. All full TNV3
files match by source, pass checker and have240 pictures/level2. Evidence:
encoder_ingest_decode.json and encoder_ingest_film.json. Input source hashes,
actual commands and film reports retained; preparation helper added.

Reject hardware ingestion default: no measured end-to-end gain. Cold ingestion
path tested; TVC hot path and production film module unchanged. Inputs are
10-second8-bit study derivatives with silence;1920x802 is upscaled800x334,
not authentic full-resolution films. No 4K/HDR10bit/energy/physical-engine
attribution, native Windows/Linux, FPGA or Pocket claims. Future gate requires
representative original high-resolution inputs and a whole-pipeline win.

Final unchanged production O3 gate passes: mean BD-rate-1.52%, bins/B9.676 vs
frozen9.618, bins/MB51.87 vs52.13, negative equal-quality bin changes all clips,
frozen vectors and recon match. Evidence encoder_study_production_gate.json.
Detailed rationale/reproduction and handoff: ENCODER_FOUR_STEP_STUDY.md.
No active encode/build jobs, VM/SD operations or push at handoff. Experimental
variants are not default-enabled; no new quality gain claimed.

## E-027 — Film process/thread contention (2026-10-30)

Host: three alternating repetitions of the 240-frame 800x334 action film,
three segments, comparing serial E-022 with the E-025 B-worker prototype.
Median seconds serial/workers: one segment job 16.315/12.787 (1.276x), two
9.606/8.125 (1.182x), four 5.357/5.741 (0.933x). Every TNV3 SHA matches and
passes the film checker. Evidence: encoder_film_workers_bench.json; reproduce
with tools/tvc_ref/enc/film_workers_bench.py after building the prototype and
preparing the E-026 film inputs. This is a short three-segment study, not an
exhaustive scheduling or energy benchmark. The hot-path prototype uses extra
cores; high segment concurrency can negate the gain. Keep it optional until an
explicit process/thread budget and portable failure fallback are implemented.
No encoder, decoder, FPGA resource/timing, or Pocket change. Next gate: test
representative longer jobs and integrate bounded scheduling, then the Rust
crate requirements supplied by Omega, subject to an agreed writable location.


## E-028 — Experimental per-process B-worker budget (2026-10-31)

The isolated generated pthread prototype now recognizes TVC_B_WORKERS=1 as a
request to use the original serial B-picture path. Unset or other values retain
the previous two-worker behavior. This allows an external segment scheduler to
reserve one CPU per encoder process; it does not itself coordinate processes,
validate arbitrary limits, create persistent workers, or affect production C.
E-027 showed why this control is needed. Choosing a budget automatically or
promoting the POSIX/TLS prototype was deferred pending representative workloads,
portable implementation and a global worker budget.

Host verification: with the limit set, all five E-025 stress cases preserve
complete streams and reconstruction versus serial and the frozen decoder,
including level retries (encoder_bthreads_budget_stress.json); the full corpus
gate passes (-0.92%, -1.58%, -2.45%, -1.13%, mean -1.52% BD-rate; mean bins/B
9.676 vs frozen 9.618, bins/MB 51.87 vs 52.13). Gate evidence:
encoder_bthreads_budget_gate.json; binary/source hashes and build command:
encoder_experiment_bthreads_budget.json. The gate is run with TVC_B_WORKERS=1;
its JSON does not itself record this environment variable.

With the limit unset, three alternating host paired runs yield action400 1.434x,
anime400 1.302x, bw400 1.337x with identical bitstreams
(encoder_speed_bthreads_budget.json). These are single-encoder measurements,
not a new whole-film throughput or power claim. No FPGA resource/timing or
Pocket changes; Windows/Linux runtime and multi-process budget integration
remain unverified. Next: explicit portable encoder context and bounded reusable
workers, with level/decoder gates and longer film benchmarks before promotion.

## E-029 — Skip unused last-position model copies in suffix trials (2026-10-31)

In the CPU RDOQ hot path, suffix replay calls only significance, greater-than-one
and remainder models, so its local context copy omits the unused last-position
model. Prefix/header and full replays retain their complete copies. This keeps
the same floating-point count order and greedy quantization decisions; no
syntax, decoder, firmware or hardware changes. An alternative was to keep the
full copy for simplicity; the new helper is small and measured twice before
acceptance. Details and evidence: ENCODER_RDOQ_PREFIX.md.

Host: 12000 synthetic differential blocks versus full replay, all trial costs,
ASan/UBSan and unchanged live contexts PASS. O3 full gate PASS: BD-rate dialog
-0.92%, action -1.58%, anime -2.45%, bw -1.13%, mean -1.52%; mean bins/B
9.676 vs frozen 9.618, bins/MB 51.87 vs 52.13. Frozen vectors and encoder vs
decoder reconstruction pass (encoder_suffix_copy_gate.json).

Three initial and five repeated paired O3 runs on action/anime/bw400 yield
identical streams. Repeat speed ratios 1.009, 1.016 and 1.010 respectively;
action800 three runs 1.003. Small differences may be noise, particularly at
800; no new compression gain. Raw speed JSONs encoder_speed_suffix_copy*.
240-frame 800x334 level-2 film plus silence matches the prior complete TNV3
byte-for-byte, passes checker, 976528 bytes, no level raises/joint retries;
encoder_film_suffix_copy.json. Not a full-length film or level3 timing claim.

A concurrent commit edffcbb5 recorded a new Pocket artifact quality target
and captured the benchmark JSON while this work was running; it is preserved.
Next investigation should reproduce the documented bottom-region artifacts and
add a per-frame spatial check before changing encoder decisions. No new FPGA
resource/timing measurement or Pocket claim from this CPU change.

## E-030 — Spatial error diagnostic and display hypothesis (2026-10-31)

The later Pocket note in ENCODER_BRIEF.md described a visible coloured bottom
band and a separate luma-error heuristic as evidence of an encoder problem.
The owner now suspects the visible band instead came from a player window
larger than the video and pixel padding. That is plausible but unverified here;
reconstruction equality does not determine the on-screen composition. The
observed luma MSE values and the visible coloured band are distinct observations;
no causal link between them is established. The previously described film
excerpt and a matching screenshot/player configuration are not present in this
encoder worktree. The note's encoder-cause conclusion remains a hypothesis,
not a confirmed hardware, codec or player diagnosis. Do not change padding or
coding decisions on that basis alone.

Added a cold-path gate diagnostic: for every decoded display-order frame, compute
raw 8-bit luma MSE on the first 100 and last 40 rows, flag only bottom > 80 and
bottom > 2*top. Frame indices are zero-based; no colour-range conversion occurs.
The threshold is heuristic, can flag legitimate spatially uneven complexity,
and cannot detect chroma-only or display-window artifacts. Results and source
SHA256 are retained per frame in encoder_spatial_baseline.json. Flags are
reported, not acceptance failures; the original quality/decoder gates remain
unchanged. This avoids rejecting legitimate content based on an unvalidated
threshold. Synthetic Y4M tests check regions, frame ordering and threshold.

Host O3 full gate PASS on current four clips x three QPs x 96 frames; no flags
in this limited corpus. Baseline BD-rate dialog -0.92%, action -1.58%, anime
-2.45%, bw -1.13%, mean -1.52%; mean bins/B 9.676 vs frozen 9.618, bins/MB
51.87 vs 52.13, reconstruction/frozen vectors pass. Evidence: the baseline
JSON and work/encoder_lab/spatial-baseline-gate.log. The tested clips are not
the Pocket film and cannot verify that observation. No encoder hot-path,
bitstream, FPGA resources/timing or Pocket behavior changed. Next: if the film
and matching player configuration become available, compare visible dimensions,
crop/window, padding and both luma/chroma source-vs-decode before assigning
cause; otherwise focus on independently measured encoder optimization.

## E-031 — Direct original-to-current speed and memory baseline (2026-10-31)

Host only, no codec/hardware changes: built original encoder from frozen
`tvc-dec-draft5` source with -O2, current E-029 source with -O3. Three
alternating 96-frame repetitions on action400 QP30, anime400 QP24 and bw400
QP36 give 1.482x, 1.376x and 1.999x throughput respectively. These are
end-to-end encodes; streams differ because E-007 improved coding choices.
`encoder_speed_total.json` retains binary/stream hashes and individual runs.
This comparison includes both algorithm and compiler optimization changes.

Single-run 240-frame measurements (not statistical performance claims):
action400 1.492x, anime400 1.521x, bw400 1.989x, action800 1.789x. Peak RSS
MiB original/current: 48.0/33.6, 64.0/40.4, 96.4/52.0, 177.5/114.9.
macOS wait4 per-child high-water mark; reading whole input remains a memory
limit. `encoder_long_baseline.json` includes hashes, sizes and raw figures;
`tools/tvc_ref/enc/bench_long.py` reproduces the cases after building the two
binaries as described above. `/usr/bin/time -l` failed under sandbox sysctl
restriction, so wait4 was used; its result is host evidence, not Pocket data.
Current quality gate against frozen baseline remains -1.52% BD-rate with
mean bins/B 9.676 vs 9.618 and bins/MB 51.87 vs 52.13 (E-029); timings are
same-QP, not equal-quality comparisons. No hardware resource/timing changes.
Next: explicit worker contexts, bounded scheduling and longer input coverage.

## E-032 — Reusable B-picture worker-pool experiment (2026-10-31)

The E-025 research variant created and joined two POSIX threads for every B
pair. This experiment adds a fixed pair of reusable pthread workers with
per-slot mutex/condition synchronization, ordered parent-side commit and serial
fallback on pool initialization failure. TVC_B_WORKERS=1 avoids creating the
pool. If worker MB-state allocation fails, its result uses serial retry. The
pool caller owns task data, waits before slot reuse and joins on shutdown; the
picture globals/scratch remain thread-local in the generated experimental
encoder. This is not yet the explicit portable context or global process/thread
budget required for production. The production encoder is unchanged. Thread
creation failure fallback was forced via TVC_BPOOL_TEST_INIT_FAIL; worker MB
allocation failure and every possible allocation error have not been injected.

Host checks: standalone pool handles 200000 tasks, including under TSan. Full
O3 corpus gate passes (-1.52% mean BD-rate, bins/B 9.676 vs 9.618 baseline,
bins/MB 51.87 vs 52.13); frozen vectors and all reconstructed frames match.
Five serial-vs-pool level/AQ/slice stress cases pass exact streams and frozen
decoder reconstructions, including level retry (encoder_bpool_stress.json).
A forced initialization failure produces identical 24-frame level1 stream and
reconstruction. A separate 24-frame TSan encoder run passes with frozen decoder
reconstruction match (work/encoder_lab/bpool-encoder-tsan.log); this is bounded
race evidence. A 240-frame level2 study film passes the TNV3 checker and matches
the complete E-022 file (encoder_film_bpool.json); not a full-length film.

Three alternating host repetitions vs serial: action400 53.10->74.91fps
(1.411x), anime400 22.24->28.91fps (1.300x), bw400 58.21->77.18fps (1.326x),
action800 20.64->29.78fps (1.442x). All stream hashes match. Against the
thread-per-picture prototype, pool ratios are 1.023, 1.002, 0.999 for the
three 400 cases: no consistent additional pool speedup established. Evidence:
encoder_speed_bpool*.json, encoder_bpool_gate.json and
encoder_experiment_bpool.json. Power and per-core efficiency are unmeasured.

Retain only as optional generated research code. Alternatives: serial remains
default; promoting an unportable pool without shared worker budget is rejected.
Remaining gates: explicit contexts instead of TLS, reusable buffer allocation
and complete allocation-failure behavior, a global segment/picture budget,
Windows/Linux native builds, longer film and broader race tests. No FPGA
resource/timing change or Pocket evidence. Reproduce with make_experiment.py
bpool, the gate with TVC_ENCODER_BINARY=work/encoder_lab/encoder-bpool, and
stress_bthreads.py with TVC_BTHREADS_BINARY set to that binary and a separate
TVC_STRESS_OUTPUT. 2026-10-30 E-025/E-027 measurements remain historical.

## E-033 — Re-profile and reject last-coefficient scan shortcut (2026-10-31)

Host O3 inclusive instrumentation after E-029 still shows RDOQ about 49% of
runtime on action400, 54% on anime400 and 37% on action800, fractional
prediction/SAD about 7–8%; timings include instrument overhead and FDCT8 omits
4x4. Evidence encoder_profile_after_pool.json. The profiling helper now records
SHA256 of all included encoder headers and frozen tvc.h, since source-only hashes
miss E-029's header change. No profiling-driven hardware or Pocket inference.

Tested starting the hot RDOQ loop at the last nonzero position rather than
visiting and skipping earlier high positions. Differential 12000-block test
with ASan/UBSan passes, every trial bit cost matches full replay, and all paired
benchmark streams match. Three alternating O3 host repetitions: speed ratios
action400 0.975, anime400 1.005, bw400 1.013. Result is mixed and action is
slower; no consistent benefit established, so reverted the candidate. Raw
results encoder_speed_rdoq_scan.json. No production algorithm change or decoder,
FPGA resource/timing, Pocket, quality or power claim. Further profiling and
cross-platform tests remain; retain E-029 as accepted RDOQ implementation.

## E-034 — Specialize count-only RDOQ prefix and suffix operations (2026-10-31)

RDOQ prefix/suffix analysis always counts rather than range-encodes or decodes.
Encoder-private count-only bit and Exp-Golomb helpers eliminate general RC mode
dispatch in that hot path; probability updates and every floating addition stay
in the same order. Full replay retains the frozen general counter as oracle.
This preserves greedy decisions, bitstream, decoder and hardware. The alternative
of leaving the dispatch was slower in the paired tests; no fast-math used.

Host: 12000 synthetic differential blocks and exact per-trial count assertions
pass ASan/UBSan and Rosetta x86-64. O3 and default O2 full gates PASS with
unchanged corpus sizes/PSNR/bins and frozen-decoder reconstruction. BD-rate
versus original remains dialog -0.92%, action -1.58%, anime -2.45%, bw -1.13%,
mean -1.52%; mean bins/B 9.676 vs 9.618, bins/MB 51.87 vs 52.13. Evidence:
encoder_count_special_gate.json, encoder_count_special_o2_gate.json.

Three alternating paired O3 host runs: action400 52.48->60.83fps (1.159x),
anime400 22.00->29.66fps (1.348x), bw400 57.64->63.25fps (1.097x), action800
20.39->22.35fps (1.096x); complete streams identical in every case. Evidence:
encoder_speed_count_special.json and encoder_speed_count_special_800.json.
240-frame action level2 film + silence passes TNV3 and complete-file cmp
against E-022: 976528B, max picture25055B, worst second160475B, no QP raises
or joint retries (encoder_film_count_special.json). Host-only study clips, not
full-length/level3 performance, power, native Linux/Windows or Pocket validation.
No FPGA resources/timing touched. Next: remeasure worker-pool combination,
portable context and shared scheduling; then continue CPU/platform evaluation.

## E-035 — Combined optimized RDOQ and B-pool evaluation (2026-10-31)

Rebuilt E-032's experimental pool against E-034's accepted count-only RDOQ.
Full host corpus gate passes, same sizes/PSNR/bins and reference reconstruction;
all five level/AQ/slice stress cases match serial streams and frozen decoder.
A 240-frame level2 action film plus silence passes TNV3 check and matches E-022
full file byte-for-byte; no QP raises/joint reencodes. Evidence:
encoder_bpool_count_gate.json, encoder_bpool_count_stress.json,
encoder_film_combined.json and encoder_experiment_bpool_count.json.

Three alternating paired O3 runs: current serial vs experimental pool on
400-pixel action QP30 59.93->85.62fps (1.429x), anime QP24 29.25->38.85fps
(1.328x), bw QP36 62.13->83.45fps (1.343x); 800-pixel action QP30
22.07->32.32fps (1.464x). All hashes identical. Evidence:
encoder_speed_combined.json and encoder_speed_combined_800.json. Pool uses more
CPU cores and retains E-032 portability/worker-budget limitations; not default.

Fresh direct original O2 vs current production O3 serial measurements, same
96 frames/settings, three alternating runs: action 34.56->59.93fps (1.734x),
anime 15.67->29.15fps (1.860x), bw 28.25->62.14fps (2.200x). Streams differ
because encoder decisions improved, and speed is same-QP, not equal-quality.
Evidence encoder_speed_total_e034.json; corpus mean BD-rate remains -1.52% and
equal-quality decoder-bin changes negative on all clips (E-034). These totals
include compilation and algorithm changes and are host-only, not native
Windows/Linux, Pocket, energy or full-length film evidence. No FPGA resource or
timing impact. Remaining: portable explicit worker contexts and process budget,
longer and heterogeneous benchmarks, architecture-specific evaluation.

## E-036 — Re-evaluate explicit SIMD after E-034 (2026-10-31)

Host M4 ARM64, O3, three alternating 96-frame repetitions, explicit NEON vs
compiler-selected portable C: action400 0.995x, anime400 0.998x, bw400 0.995x,
action800 1.001x. All paired streams identical; differences are tiny and do not
establish a speed benefit. 20000 SAD differential comparisons pass ASan/UBSan,
including borders and fractional vectors. Explicit NEON corpus gate passes with
E-034's unchanged rate/PSNR/bin results. Evidence encoder_speed_simd_e034*.json,
encoder_neon_e034_gate.json and work/encoder_lab/simd-e034-check.log. Keep
portable C default; explicit SIMD remains opt-in. This addresses encoder pixel
hot path only; no new FPGA resource/timing or Pocket change.

An x86-64 SSE2 O3 build ran the same 20000-case SAD test and full corpus gate
under Rosetta on this host. Gate passes, mean BD-rate -1.59% against the ARM
frozen baseline, mean bins/B 9.676 vs 9.618, bins/MB 51.85 vs 52.13; the
floating analysis can differ across architectures, so do not claim bit-identical
streams across ARM/x86. Evidence encoder_sse2_e034_rosetta.json and
work/encoder_lab/sse2-e034-check.log. Rosetta is correctness evidence only,
not native x86 throughput, Linux or Windows validation. No runtime ISA dispatch,
AVX2, native Intel/AMD performance or energy claim. Further native-platform
measurements required before promoting specialized paths. Restored production
encoder binary to native O3 after the checks.

## E-037 — Bound look-ahead propagation storage (2026-10-31)

The backward propagation pass only reads the current frame and adds to the
preceding frame. Calculate each frame's final offsets after its incoming
propagation is complete, then reuse two zeroed propagation buffers. This changes
propagation storage from n*macroblocks*sizeof(double) to 2*macroblocks*sizeof(double);
per-target addition order and final offset calculations are unchanged. This is
encoder look-ahead (cold analysis path); all source frames, other look-ahead
arrays and output offsets are still held, so total memory remains O(n). No
bitstream, decoder, hardware or quality changes. Full-history propagation was
retained as a standalone differential oracle rather than removed without proof.

Host: 200 randomized sequences with different scene cuts, vectors and qpel modes
match all offset bytes under ASan/UBSan (check_tree.c). Five stressed level/AQ/
slice cases preserve full streams and frozen-decoder reconstructions. O3 and O2
corpus gates PASS with unchanged -1.52% mean BD-rate, 9.676 vs 9.618 bins/B,
51.87 vs 52.13 bins/MB. Three paired O3 400-pixel timing ratios 0.995, 0.997,
0.999 are within ordinary run variation; no speed gain claimed. On one 240-frame
run per input, peak RSS before/after MiB: action400 33.78/33.53, anime400
40.61/40.11, bw400 51.98/51.45, action800 114.83/114.11. These are single
host observations and depend on allocator/page behavior. All paired streams are
identical. 240-frame level2 film + silence passes TNV3 check and is byte-identical
to E-022; no raised QPs/joint retries. Evidence: encoder_tree_memory_gate.json,
encoder_tree_memory_o2_gate.json, encoder_tree_memory_stress.json,
encoder_speed_tree_memory.json, encoder_tree_memory_long.json,
encoder_film_tree_memory.json. bench_long.py and stress_bthreads.py accept
in-worktree comparison-binary overrides for reproduction. Next gate for larger
memory savings is bounded segment/input processing without changing look-ahead
or level-window decisions; full-length and native Windows/Linux memory remain
untested. No FPGA resource/timing or Pocket change.

## E-038 — Release completed look-ahead arrays before picture encoding (2026-10-31)

After scene cuts and tree offsets are computed, intra/inter costs and motion
vectors are no longer used. Free the four Look arrays before AQ and picture
encoding. This is a cold-path lifetime change; it does not change look-ahead
calculations, syntax, quantization, decoder, RTL or firmware. The alternative
of retaining them until process exit wastes memory while reconstruction frames
accumulate. The encoder still holds the full source and optional reconstructions;
this is not a streaming or bounded-total-memory solution.

Host O3 corpus gate passes with unchanged -1.52% mean BD-rate, bins/B 9.676 vs
9.618 and bins/MB 51.87 vs 52.13; decoder recon/frozen vectors pass. Five
level/AQ/slice stress cases match prior streams and reference decoder. ASan/UBSan
12-frame tree-off/AQ-on run passes and recon matches decoder. Four paired
240-frame runs with reconstruction have identical complete stream and recon
hashes. Single-run max RSS MiB before/after: action400 62.23/60.69, anime400
72.48/72.59, bw400 96.48/93.80, action800 215.27/204.61. RSS varies with
allocator/high-water behavior; no claimed saving for the anime case or broad
speed improvement. Evidence encoder_free_look_long.json,
encoder_free_look_gate.json, encoder_free_look_stress.json and
work/encoder_lab/free-look-asan.log. bench_long.py now supports -r measurement.

Level2 240-frame film + silence passes TNV3 and complete-file comparison with
E-022; no level raises/joint retries (encoder_film_free_look.json). No native
Windows/Linux, full-length, energy, FPGA resource/timing or Pocket evidence.
Next: measure bounded source-frame storage across segments and coordinate film
segment concurrency with the experimental picture workers.

## E-039 — Explicit film encoder-slot budget (2026-10-31)

The film helper accepts optional --cpu-budget N and --encoder-threads 1|2.
It limits concurrent segment TVC jobs to min(--jobs, floor(N/threads)), rejects
invalid budgets before probing input, and passes TVC_B_WORKERS to segment jobs
and any level-window joint retry. The default invocation retains existing job
selection and inherited environment. This is cold-path orchestration; the
budget accounts for TVC encoder slots only, not ffmpeg, audio, the parent or OS
threads. --encoder-threads is a requested setting for encoders that implement
TVC_B_WORKERS; the shipped serial encoder ignores it, while the optional POSIX
pool prototype implements it. The report records the requested setting, actual
job limit and budget, not proof of hardware CPU occupancy. No automatic choice
of segment vs picture parallelism is claimed. No decoder or format change.

Host boundary tests cover zero/invalid budgets and job clamps; Python compile
passes. Three alternating repetitions per configuration on 240 action study
frames, three segments, level2 and experimental E-032 pool: budget2 with two
one-thread jobs 9.310s vs one two-thread job 12.017s; budget4 with four
one-thread jobs 4.910s vs two two-thread jobs 7.764s. With only three segments,
four is an upper limit, not four simultaneous jobs. All 12 complete files
match by SHA and pass the TNV3 checker. encoder_film_budget.json retains reports,
settings and timings; encoder_experiment_bpool_budget.json records binary hash.
Study-only finding; no full-length/other-host/energy or optimal-policy claim.
Default production film invocation also passes checker and matches E-022 full
file (encoder_film_budget_default.json). No joint re-encode was triggered in
these trials, so passing the budget to that path is code-reviewed, not a tested
forced-joint result. Native Windows/Linux and portable pool integration remain
open. No FPGA resource/timing or Pocket change.

## E-040 — Per-job picture context prototype (2026-10-31)

The E-032 generated research encoder used separate thread-local Pic, SRC, CUR,
reference pointers and scratch arrays. An additional generated `bcontext`
variant packages these fields in an EncPictureContext owned by each B job, plus
a parent context. A thread-local pointer binds existing helper functions to the
active context; it is an adapter, not explicit context arguments throughout.
The POSIX pool and caller-owned immutable references remain. Parent joins before
reference reuse and commits in order. Alternative fully refactoring every helper
was deferred pending this isolated correctness/performance evidence. Production
C remains unchanged; no decoder, FPGA resource/timing or Pocket changes.

Host O3 full gate passes (-1.52% BD-rate mean; bins/B 9.676 vs 9.618; bins/MB
51.87 vs 52.13). Five level/AQ/slice cases compare exact streams and frozen
decoder reconstructions. A 24-frame TSan encode matches frozen decoder; forced
pool init failure matches serial stream/recon. Level2 240-frame film + silence
passes TNV3 and complete-file cmp against E-022. Evidence:
encoder_bcontext_gate.json, encoder_bcontext_stress.json,
encoder_film_bcontext.json, encoder_experiment_bcontext.json, and
work/encoder_lab/bcontext-tsan.log and bcontext-fallback.log. TSan/failure
coverage is bounded and does not test all allocation failures.

Three paired O3 host runs vs current serial: action400 59.54->84.71fps 1.423x,
anime400 28.88->38.78fps 1.343x, bw400 61.41->82.40fps 1.342x, action800
21.94->32.03fps 1.460x; all streams identical. Vs E-032 pool 400 ratios
1.004, 0.998, 1.020: no consistent separate gain established. Raw data:
encoder_speed_bcontext*.json. No native Linux/Windows, full-film/power claim.
The generated code retains TLS binding, POSIX-specific pool, unhandled buffer
allocation failures, per-job allocation overhead and no automatic coordination
beyond the opt-in E-039 slot budget. Next: explicit function-context parameters,
portable backend, robust failure paths, repeated full gates and longer films
before production integration; serial remains default.

## E-041 — Explicit picture-context argument prototype (2026-10-31)

E-040 isolated mutable picture data into per-job objects but retained a TLS
binding. A new generated `bargs` research variant passes EncPictureContext*
through the 18 encoder helper functions that access picture state, directly or
through their callees; worker and main own separate objects. There is no
_Thread_local state in generated code. The pool still uses POSIX pthreads,
shared initialization tables are read-only during encoding, and rate history
is parent-owned. The research converter checks expected insertion points and
call graph but is not a general C parser; it must not be treated as a production
refactor without manual review and explicit call sites. Alternatives: keep TLS
adapter (E-040), or production-wide refactor; prototype chosen to measure cost
and correctness before promotion. No production C, decoder, FPGA or Pocket
change; hot-path calling convention changes only in the generated variant.

Host O3 full corpus gate passes (-1.52% mean BD-rate, bins/B 9.676 vs 9.618,
bins/MB 51.87 vs 52.13); five stressed level/AQ/slice cases have identical
streams and frozen-decoder reconstructions. A 24-frame TSan run matches decoder;
forced pool init failure matches serial stream/recon. Level2 240-frame film +
silence passes TNV3 check and full-file cmp vs E-022. Evidence:
encoder_bargs_gate.json, encoder_bargs_stress.json, encoder_film_bargs.json,
encoder_experiment_bargs.json, work/encoder_lab/bargs-tsan.log and
bargs-fallback.log. Bounded race/failure tests do not cover every configuration.

Three paired O3 host runs vs current serial: action400 62.11->88.81fps 1.430x,
anime400 30.10->40.21fps 1.336x, bw400 64.37->86.83fps 1.349x, action800
22.95->33.47fps 1.458x; streams identical. Against E-040 TLS adapter the
400-pixel ratios 1.001, 1.001, 0.997 show no meaningful speed difference.
Raw: encoder_speed_bargs*.json. POSIX-only, unhandled allocations, reuse limits,
no native Windows/Linux, full-length film or power evidence remain. E-039 lets
an external film job choose an encoder-slot budget, but automatic scheduling is
not established. Next: manually integrate explicit contexts with a portable
worker backend and failure cleanup, then repeat all gates before enabling.

## E-042 — Isolate explicit context cost in serial encoder (2026-10-31)

Generated `bserialctx` from E-041's explicit context variant with the pthread
pool and worker branches removed. The generated serial candidate does not use
thread-local storage or pthreads; it passes the context through 18 helpers.
This isolates the production-refactor cost from parallel speedup. Production C
remains unchanged: replacing it with mechanically generated source was deferred
until a manually reviewed portable worker backend and failure path are ready.
The call-graph converter is encoder-specific and not a general C parser.

Host O3 corpus gate and five level/AQ/slice stress cases pass, with exact prior
streams and frozen decoder recon. 240-frame level2 film plus silence passes
TNV3 and full-file cmp against E-022. A 24-frame x86-64 Rosetta O3 comparison
matches the x86 serial stream/recon and frozen decoder. Evidence:
encoder_serialctx_gate.json, encoder_serialctx_stress.json,
encoder_film_serialctx.json, encoder_experiment_serialctx.json and
work/encoder_lab/serialctx-x86.log. Native Windows/Linux remains untested.

Three alternating paired host O3 runs vs current serial give speed ratios 0.993
(action400), 0.987 (anime400), 0.988 (bw400), 0.997 (action800), all exact
stream hashes; raw encoder_speed_serialctx*.json. Small possible 0.3–1.3%
serial regression should be considered alongside the 33–46% experimental
picture-worker gain, not misrepresented as an isolated optimization. No new
quality, decoder, FPGA timing/resource or Pocket change. Remaining: manually
review explicit contexts, portable worker implementation, safe allocation and
failure handling, global budget and long-running platform tests.

## E-043 — Windows worker backend model (2026-10-31)

Added optional Win32 worker-pool branch using CRT-compatible _beginthreadex,
critical sections and condition variables. The pthread branch remains for
macOS/Linux and production encoder stays serial. Worker slots are caller-owned;
startup failure joins already-created workers and enables serial fallback.
Design references and limitations: ENCODER_WIN_WORKERS.md. Alternatives were
C11 threads (threads.h absent in this host SDK), or shipping pthread-only; the
Windows branch remains research because no native Windows SDK/runtime exists here.

Host simulation only: a test-only pthread-backed Win32 API model passes 200000
slot reuse operations, first/second worker creation failure cleanup and a bounded
TSan run. The explicit-context encoder compiled against this model passes O3
corpus gate (-1.52% mean BD-rate, bins/B9.676 vs9.618, bins/MB51.87 vs52.13),
five level/AQ/slice stress cases, serial fallback on each simulated startup
failure, frozen decoder reconstruction, and 240-frame level2 film full-file
cmp. Evidence encoder_winmock_gate.json, encoder_winmock_stress.json,
encoder_film_winmock.json and work/encoder_lab/winmock-tsan.log. The real
POSIX pool standalone test was rechecked. Neither simulated API nor Rosetta is
native Windows/Linux validation. No native build, OS scheduling or throughput
claim, and no hardware/FPGA timing/Pocket change. Next gate: real Windows and
Linux toolchains and runtime, pool failure injection, full gates and long films
before any production promotion.

## E-044 — Reject global AQ and look-ahead strength changes (2026-10-31)

Tested encoder-only decision settings on the standard 4 clips x 3 QPs, 96
frames, current O3 build. Compared each against both frozen baseline and the
current accepted E-038 baseline (encoder_free_look_gate.json). No source or
default-setting change; these are analysis hot-path choices. All reconstructions
matched the frozen decoder. BD-rate below is versus current accepted encoder,
clip order dialog/action/anime_dialog/bw, followed by mean:

- AQ 0.25: +0.37%, +2.50%, +2.52%, -1.05%; mean +1.09%.
- AQ 0.5: +0.77%, +5.33%, +5.29%, -1.51%; mean +2.47%.
- AQ 1.0: +3.11%, +13.51%, +14.39%, -0.15%; mean +7.71%.
- Tree 1.0 instead of 2.0: +1.66%, +1.67%, +1.31%, +2.97%; mean +1.90%.
- Tree 3.0 instead of 2.0: +2.49%, +0.37%, +4.23%, +2.56%; mean +2.41%.

All five fail both the frozen-baseline gate and the current comparison. AQ
0.25/0.5 improves bw_room but raises action/anime equal-quality bin cost; the
other settings also fail per-clip quality/cost criteria. Per-clip interpolation
and parser bins are in encoder_aq_*.json and encoder_tree_*.json; logs:
work/encoder_lab/aq-*.log and tree-*.log. Three QPs and four short clips are
not subjective/perceptual quality evidence and do not disprove content-specific
AQ. No film test was justified for rejected settings. Defaults remain AQ 0,
tree 2. No decoder, FPGA resource/timing, native Windows/Linux or Pocket change.
Next: test other encoder decision strategies only with the same gates.

## E-045 — B-picture count quality/cost trade-off (2026-10-31)

Tested one and three B pictures against the accepted two, on the standard four
96-frame clips and three QPs. Encoder-only hot-path decision experiment; no
production source or defaults changed. Reconstructions match frozen decoder.
Relative to current accepted encoder, B1 BD-rate changes dialog +9.89%, action
+0.77%, anime +5.77%, bw +11.19%, mean +6.90%; it fails. B3 changes dialog
-5.97%, action +0.41%, anime +0.85%, bw -4.87%, mean -2.39%, but increases
interpolated equal-quality decoder bins on action by 1.39% and anime by 1.80%,
so it fails the decoder-cost rule despite mean size gains. Both also fail the
frozen-baseline gate. Raw results encoder_bcount_1.json and encoder_bcount_3.json;
logs work/encoder_lab/bcount-1.log and bcount-3.log. No level film run warranted
for rejected global settings. Mean three-point BD-rate does not establish visual
quality; no Pocket or FPGA resource/timing change.

A quick host diagnostic of the existing average source look_ratio at QP30 gives
dialog .144, action .366, anime .134, bw .137. A simple low-motion rule cannot
separate anime (which fails B3 decoder cost) from dialog/bw (which benefit). These
numbers use only four short clips; choosing a threshold on them would overfit.
No adaptive rule was promoted. More diverse independent clips and per-segment
quality/cost validation are needed before revisiting adaptive B count.

## E-046 — Worker preparation failure fallback (2026-10-31)

The E-041 generated explicit-context pool prepared and launched each job in a
single loop, with unchecked job reconstruction/offset allocations. The `bsafe`
research variant prepares both jobs before launching either. If any plane or
offset allocation fails, it releases the partially prepared pair and encodes
those pictures serially in order. Worker MB-state allocation failure and pool
startup failure also use serial fallback. Test-only environment switches inject
failure at either preparation slot, worker MB allocation and pool initialization.
The existing serial encoder itself still has unchecked allocations, so fallback
under genuine process-wide exhaustion is not guaranteed; range-coder and other
allocations remain unhandled. This is not production-ready failure handling.
The generated explicit-context prototype is optional; no decoder, FPGA or Pocket
change. Preflight-before-submit was chosen to avoid leaving one live job while
falling back after failure in the other.

Host 24-frame level1 AQ-on injected failures at prep1, prep2, worker MB and
startup all match serial stream/recon SHA (encoder_bsafe_faults.json). O3 corpus
gate passes unchanged -1.52% mean BD-rate, bins/B9.676 vs9.618, bins/MB51.87
vs52.13. Five level/AQ/slice stress cases match serial and frozen decoder; a
24-frame TSan run matches decoded reconstruction. Level2 240-frame film plus
silence passes TNV3 and full-file cmp vs E-022. Evidence encoder_bsafe_gate.json,
encoder_bsafe_stress.json, encoder_film_bsafe.json, encoder_experiment_bsafe.json
and work/encoder_lab/bsafe-tsan.log. These are bounded host tests, not proof for
all allocation failures or native Windows/Linux.

Three alternating O3 host runs vs serial: action400 57.68->84.03fps 1.457x,
anime400 28.22->38.07fps 1.349x, bw400 60.91->82.90fps 1.361x, action800
22.94->33.43fps 1.457x; all streams identical. Against E-041 variant, 400-pixel
ratios 1.009, 0.998, 0.993 show no consistent overhead or gain. Raw
encoder_speed_bsafe*.json. CPU/power and native-platform claims remain pending.
Next: robust all-allocation error handling and portable real-platform validation,
then manually integrate contexts/workers; the default stays serial.

## E-047 — Picture-QP offsets and held-out check (2026-10-31)

Tested I-picture offsets 2 and 4 vs default 3, and B-picture offsets 1 and 3
vs default 2, on four standard clips at QP24/30/36. Encoder-decision hot-path
settings only; no source or default change. All four pass the frozen-baseline
gate but passing that historical gate is insufficient to establish improvement
against the current encoder. Relative to E-038, I2 mean BD-rate +0.07%, I4
+0.05%, B1 +0.15%; each increases equal-quality bins on some clips and fails
current comparison. B3 mean -0.01% vs current, dialog +0.34%, action -0.35%,
anime +0.02%, bw -0.03%; estimated equal-quality bin changes are negative on
all four and the formal comparison passes. The size gain is extremely small
and per-clip mixed, so a holdout was run rather than changing the default.

On 96 frames each of four additional read-only study clips, B3 vs B2 BD-rate /
equal-quality bin change: anime_motion -0.640%/-0.642%, anime_wide
+0.030%/+0.029%, bw_garden -0.332%/-0.227%, dark -0.039%/-0.237%.
The anime_wide bin increase violates the no-increase rule at these measured
points; its tiny magnitude and the tiny four-clip mean gain do not establish a
reliable quality benefit. Do not enable B3 by default. All holdout streams
reconstruct exactly with the frozen decoder; no level/film acceptance claimed
for a rejected setting. Raw files encoder_offset_{i2,i4,o1,o3}.json and
encoder_offset_holdout.json; reproduction tools/tvc_ref/enc/eval_offset_holdout.py;
logs work/encoder_lab/offset-*.log. PSNR-based three-point estimates on short
clips are not subjective quality measurements. No hardware/FPGA timing, Pocket,
Windows/Linux or power changes. Next: use broader independent films before
reconsidering content-adaptive offsets or any marginal default change.

## E-048 — Final current-state validation and total comparison (2026-10-31)

Full production O3 gate PASS including no-reconstruction stream equality;
mean BD-rate -1.52% vs frozen, bins/B 9.676 vs 9.618, bins/MB 51.87 vs 52.13,
equal-quality bin changes negative per clip. Frozen decoder vectors and all
reconstructions pass. Evidence encoder_final_gate.json. Final 240-frame action
study films, level1 400x166 and level2 800x334, pass TNV3 checks and complete
file SHA match prior accepted outputs (encoder_final_films.json). No new Pocket,
FPGA resource/timing or full-length-film claim.

Three alternating paired host 96-frame runs direct original O2 vs current O3:
action400 35.13->60.39fps 1.719x, anime400 16.03->29.46fps 1.838x, bw400
28.86->63.09fps 2.186x; action800 11.68->22.65fps 1.939x. These include O2/O3
and encoder-decision changes; same-QP not equal-quality. Separate original vs
experimental bsafe-worker runs give 2.480x, 2.490x, 2.988x on the 400 cases;
worker uses additional cores and is not default. Separate serial vs worker 800
is 22.61->33.27fps 1.472x with identical streams. Raw samples/binary hashes in
encoder_final_speed*.json. See ENCODER_TOTAL_RESULTS.md for exact limits,
accepted/rejected work, and outstanding platform, resource, scheduling and
full-length validation gates. Final differential RDOQ/tree sanitizers, spatial,
budget and 200000-task pool tests and path guard pass. No active jobs, push,
VM or SD operation at handoff; later work must not interpret this measurement
round as proof that every optimization or native platform gate is complete.

## E-049 — More B pictures and four workers (2026-10-31)

Compared B4/B6 with accepted B2 on four clips, three QPs. Both fail the frozen
and current quality/cost gates. Versus current, B4 mean BD-rate -3.12% but
+3.59%/+5.66% equal-quality bins on action/anime; B6 mean -1.40% but +7.87%,
+13.68%, +2.17% bins on action/anime/bw and >+1% BD-rate on action/anime.
This extends the rejected B3 experiment E-045; size gains on quiet clips do
not justify increasing decoder cost on others. encoder_bcount_4/6.json.

A generated four-worker explicit-context pool (`bwide`) runs B pictures in
batches of four. Against two workers with the same B count and exact streams,
three alternating 48-frame host runs show B4 action/anime 1.427x/1.341x, B6
1.336x/1.277x; B6 action800 1.340x. More workers use more CPU; B4/B6 remain
rejected for quality/cost. A requested TVC_B_WORKERS value other than 4 forces
serial to avoid bypassing a two-slot film budget. Separate B4/B6/level3
stress cases pass, including level retries and all four injected preparation
failures. Four-worker pool passes 240000 total reuse tasks (two/four modes),
mocked Windows worker 3/4 startup failures, and a bounded B6 TSan run. A
240-frame level1 B6 stream/recon matches serial and frozen decoder, video-only
TNV3 valid (encoder_bwide_level1_study.json). Evidence encoder_bwide_stress.json,
encoder_speed_bwide_*_final.json and encoder_experiment_bwide_parallel.json.
No native Windows/Linux or full-length film evidence; more complete allocation
failure handling and system-wide budget needed. No production codec, FPGA
resource/timing or Pocket change.

## E-050 — Motion candidate and slice parallel prototypes (2026-10-31)

A generated `bmotion` variant runs the two B-macroblock motion searches in
parallel, joining before scoring. Paired 48-frame host runs retain exact streams
but are 0.933x action and 0.970x anime serial throughput. A bounded 12-frame
TSan/decode check passes. Rejected: no full gate/film or default change.
Evidence encoder_motion_parallel_speed.json, encoder_experiment_bmotion_parallel.json.

A generated `bslice` variant runs existing slices with private contexts and
shared disjoint image/MB rows, then joins before whole-picture deblock. Against
serial with the same layout and exact streams, three host repetitions give
1.695x action400, 1.628x anime400, 1.735x bw400 at -S3; action800 -S7 1.404x;
action800 level3 automatic two slices 1.822x. Both -S3 serial/parallel corpus
gates fail vs current: +9.26% mean BD-rate, action equal-quality bins +0.76%.
Thus do not add slices just to gain speed. A 240-frame 800x334 level3 auto-slice
video-only TNV3 passes checker, serial stream/recon and frozen decoder cmp;
all five level/AQ/slice stress cases and a bounded -S3 TSan run pass. Evidence
encoder_speed_slices*.json, encoder_slice_*_gate.json,
encoder_slice_stress.json, encoder_slice_level3_study.json and
encoder_experiment_bslice_parallel.json. Already-required slices are a promising
candidate. Separate contexts, portable backend, race/failure tests, memory and
combined segment/picture/slice budgeting remain before production. Tests are
host only, not native Windows/Linux or Pocket; no FPGA resource/timing change.

## E-051 — Look-ahead parallelism (2026-10-31)

Fresh host inclusive phase profile puts look_costs at 1.3–2.2% of four QP30
cases (source/header hashes in encoder_parallel_phase_profile.json); perfect
removal would save at most roughly that fraction under the measured workload.
Other look-ahead work is outside that timer. A generated `blook` prototype splits
independent frame analysis over two workers, then retains serial tree propagation.
Five paired 400-pixel repetitions give 1.020x action, 1.018x anime, 1.016x bw;
three action800 runs 1.011x, with identical streams. Small differences may
include noise and use extra cores. Full O3 gate PASS with unchanged -1.52%
BD-rate/bins, five stressed streams/frozen decoder checks, bounded 24-frame
TSan and level2 240-frame film cmp pass. Evidence encoder_speed_look_parallel*,
encoder_look_parallel_gate/stress.json, encoder_film_blook_parallel.json and
encoder_experiment_blook_parallel.json. Retain optional research code; no claim
of system-wide or energy benefit. No production, FPGA timing/resource or Pocket
change; native Windows/Linux and budgeted combinations remain untested.

## E-052 — Segment and pipeline scheduling study (2026-10-31)

Made a 960-frame input by repeating the same 10-second study clip four times;
this is synthetic repetition, not a longer diverse or full-length film.
Two alternating runs per config, total film times with E-039 TVC-slot budget:
96-frame segments, budget2/one thread 21.79s, budget4/one thread 18.63s,
budget4/two threads 21.59s; 480-frame segments respectively 24.39s, 17.18s,
22.58s. For each segment size, all output SHA hashes match and TNV3 check passes;
different segment sizes produce different output. Two 480-frame video jobs
exist even if executor permits four; audio is also scheduled in this executor,
so the budget4 effect is not solely a video parallelism result. Budget excludes
ffmpeg/audio/OS threads. Encoder/source hashes, full reports and individual
runs: encoder_pipeline_repeated.json, tools/tvc_ref/enc/bench_pipeline.py.

A one-video-job overlap vs synchronous-executor comparison (two runs each)
measures 31.31s vs30.83s, identical files, no demonstrated benefit for this
cheaply decoded silent repeated input; synchronous mode changes audio scheduling
as well. encoder_pipeline_overlap.json, bench_overlap.py. First attempted
pipeline run overlapped another validation encode; interrupted, verified no
remaining processes, then repeated all reported measurements without competing
encodes. Full native films, longer/diverse source, energy and Windows/Linux
remain unmeasured; no default scheduling choice changed. Neither FPGA timing nor
Pocket touched. Detailed conclusions/reproduction: ENCODER_PARALLEL_HYPOTHESES.md.

## E-053 — Parallel-hypothesis handoff gate (2026-10-31)

Final unchanged production O3 gate passes, including reconstruction/no-recon
stream equality, frozen decoder vectors and parser-cost checks: mean BD-rate
-1.52%, bins/B 9.676 vs frozen 9.618 and bins/MB 51.87 vs 52.13. Evidence
encoder_parallel_final_gate.json. Python helper syntax, pool unit tests and
encoder path guard pass; no production encoder or decoder changes in E-049–053.
The new slice, look-ahead, B-wide and motion variants remain generated research
code. No active encodes/builds at handoff, no push, VM or SD operation. The
measured results and all remaining native-platform, long-film, memory,
allocation, worker budget and Pocket gates are in
ENCODER_PARALLEL_HYPOTHESES.md. No claim that every parallel configuration or
source is covered.

## E-054 — Extract and publish TNV3 codec tools alpha (2026-10-31)

User directed a new public `tnv3` project and `Tau codecs` folder, and asked
that audio and wrapper be included. Extracted the validated serial TVC encoder,
frozen reference decoder, TAC encoder/decoder and fixed-point model, TNV3
mux/demux/check, film wrapper and format snapshots into an independent MIT
source project. Retained original code, upstream notices and source SHA manifest;
no source removal from Tau V. Created opt-in slice-parallel release build and
film flag with explicit TVC-slot budget plus one-worker/initialization-failure
fallback; no new slices. The pool is experimental and retains platform and
allocation limitations. GitHub repository and prerelease published at
https://github.com/alfatreze/tnv3/releases/tag/v0.1.0-alpha.1.

Host: clean extracted source and macOS-arm64 binary packages pass synthetic TVC
serial/slice byte equality, decoder recon, TAC reference/fixed-point audio
sample counts/transform self-test, TNV3 mux/demux/check and fallback tests.
Serial extracted binary full O3 corpus gate passes -1.52% mean BD-rate,
bins/B9.676 vs9.618, bins/MB51.87 vs52.13, including no-recon equality.
Sliced extracted binary matches serial sliced corpus; the forced-slice quality
gate against unsliced fails as documented. A generated 24-frame 800x480 level3
film plus audio passes TNV3 with byte-identical serial, one-worker fallback and
two-worker slice files. GitHub Actions Ubuntu/macOS smoke passes for release
commit 54a577c6511d9fa3453a6d5184b054ebbba1a956. No new Pocket or FPGA
resource/timing evidence; full Linux gate, Windows native, long films and old
macOS remain pending. Apple Silicon binaries target macOS12, tested on 26.6.2,
not independently signed/notarized.

Release assets and SHA, repository-boundary decision, provenance, remaining
risk and Rust crate roadmap: ENCODER_RELEASE_SPLIT.md. The Rust crate is not yet
implemented. The public source snapshot retains HarpMudd/Tau MIT notices; real
media-derived vectors were not redistributed pending rights review. Initial
external commit source snapshot is 637b9f83, with isolated release integration.
No Tau V decoder/RTL/fw, hardware VM or SD changes, and no push to the Tau V
repository. No active build/encode jobs at handoff.

### E055 — 2026-10-31: research release and local media

Decision: publish format research, measurement data and archival study scripts
in tnv3 alpha.2 alongside the four unchanged format snapshots. Historical
experiments are distinguished from current specifications; absolute local paths
were replaced with placeholders. Keep the media-derived vectors and research
media in the new project's Git-ignored local-media directory, rather than the
public repository or release archives. Neither codec hot paths nor hardware
are changed. This extends E054 without changing its conclusions.

Verification: **host** synthetic serial/slice TVC, TAC and TNV3 smoke passed;
196 research JSON files parsed; all 385 source-archive files matched the
release commit and excluded local-media. GitHub Ubuntu/macOS CI passed at
c1b94b5d8f83e4dd1f610beb9e0acd4faf723098. **code-review**: all six tnv3
commits use Abel Santos as author and committer, with no agent attribution.
383 generated experiment media files (2.52 GiB), reference vector directories
and the excluded screenshot were copied locally. No new Pocket, Quartus or
performance measurements; E054's validation limits and Rust implementation
remain the next gates. Release: https://github.com/alfatreze/tnv3/releases/tag/v0.1.0-alpha.2.

# Mac GPU and Neural Engine assessment

## Subsequent measured evaluation

E-024/E-026 execute VideoToolbox motion proposals and compressed ingestion on
real study frames. Neither improves throughput here, so both remain disabled.
Calibration failures, pixel/stream equivalence checks and full film timings are
in [ENCODER_FOUR_STEP_STUDY.md](ENCODER_FOUR_STEP_STUDY.md). The earlier assessment
below is retained as the decision history.

## E-017 refinement: VideoToolbox motion estimation

The earlier E-016 assessment below missed Apple's public macOS 26
`VTMotionEstimationSession` API. It returns motion-vector buffers from frame
pairs and accepts 4x4 or 16x16 search blocks (16x16 default), according to the
installed Apple SDK headers. This supplies proposals without training our own
model and is now a more concrete next experiment than writing a Metal search
backend. Earlier statements requiring a new learned model apply to a custom
Core ML approach, not every Apple-provided motion-analysis route. Accelerator
placement behind this session has not been established here; no ANE-use claim
is made.

On this M4 Pro running macOS 26.6.2, sessions with 16x16 blocks successfully
created and returned source-buffer attributes at 400x176, 800x336 and 1920x1080.
No frames were submitted. Therefore vector correctness, format/units, direction,
latency, hardware placement and encoder rate/quality effects remain unmeasured.
The sandbox returned `kVTSessionMalfunctionErr` (-17691); authorized execution
outside it succeeded. Raw results: `encoder_videotoolbox_probe.json` and
`encoder_videotoolbox_sandbox.json`.

VideoToolbox can also decode compatible compressed source formats before
scaling/YUV conversion and TVC encoding. This host reports hardware decode
support for H.264 and HEVC; no source-decoding throughput benchmark was run.
The gate corpus is already uncompressed Y4M, so it has no such decoding work
to offload. Hardware source decoding must preserve intended pixel range,
precision and frame timing through conversion and pass end-to-end validation.

Its standard compression/decompression sessions do not supply an implementation
of this project's custom TVC codec. Substituting an H.264/HEVC output would
require a different decoder/container plan outside this branch's frozen
contract. Wrapping a custom software codec in an OS interface would not itself
make the hardware media engine implement TVC.

Next experiment: submit known translated frame pairs through this motion API,
verify the returned vector layout/units/direction, measure per-frame latency,
then test its vectors as optional CPU-checked TVC search seeds. Existing syntax
and final rate-distortion/coding stay unchanged; full corpus/parser-cost and
level-enforced film gates are required before accepting any implementation.

Reproduce the creation-only probe on macOS 26+:

```sh
clang -O2 -fblocks -fobjc-arc -framework Foundation -framework VideoToolbox -framework CoreVideo -framework CoreMedia -o work/encoder_lab/vt_probe tools/tvc_ref/enc/videotoolbox_probe.m
work/encoder_lab/vt_probe
```

Primary sources: [Apple VideoToolbox overview](https://developer.apple.com/documentation/videotoolbox),
[Apple motion-estimation API](https://developer.apple.com/documentation/videotoolbox/vtmotionestimationsession-api-collection),
and installed `VTMotionEstimationSession.h`, `VTMotionEstimationSessionProperties.h`
and `VTErrors.h` from the Xcode macOS SDK. The SDK explicitly marks this API
available from macOS 26 and documents the supported block sizes.

## Decision

Keep the existing CPU fast path. The first plausible GPU experiment would be
frame-batched integer motion-candidate analysis, after measuring how much of
the main encoding stage it could replace. Defer a Neural Engine path until
there is a justified learned proposal model and an evaluation/training plan.
No accelerator backend is added to the production encoder by this study.

The current Mac exposes an Apple M4 Pro GPU with unified memory. The sandbox
initially returned no Metal device; an authorized run outside the sandbox
successfully queried it and ran the compute probe. This is an execution-context
restriction, not evidence of missing hardware.

## Measured CPU opportunity

These phase measurements predate the accepted E-020 fractional-kernel and E-022
RDOQ speedups. Their exact percentages are not a profile of the current encoder.

An instrumented temporary copy of the `-O3` encoder timed source/setup,
look-ahead, and main encoding. At QP30, 96 frames each of action_400,
anime_dialog_400, bw_room_400 and action_800:

- Source look-ahead accounted for approximately 0.8–1.4% of total time in the
  verified profiles. Raw results are in `docs/encoder_gpu_phases.json`.
- On those profiles, eliminating look-ahead entirely has an estimated total
  throughput ceiling of roughly 1.4% improvement. This Amdahl bound assumes
  zero accelerator overhead and no other phase changes.
- Main encoding averaged roughly 64–89 microseconds per macroblock in the
  verified profiles; this is a whole-stage average, not an individual kernel
  timing. The JSON records the exact values.

These are coarse single profiles, not repeated statistical benchmarks or a
fine-grained hotspot attribution. Verification compares every instrumented
stream with the actual production encoder. The profile writes a generated C
file only under work/, and fails if its timing insertion points drift.

```sh
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py build
python3 tools/tvc_ref/enc/profile_phases.py --verify --out work/encoder_lab/phases.json
```

Source-only look-ahead is highly parallel across blocks/frame pairs, but its
small measured share makes it a low-value standalone offload target. Its
integer cost calculation could be reproduced exactly on a GPU; no such TVC
GPU implementation was benchmarked here.

## Measured Metal submission cost

The optional Metal probe executes one integer increment through a shared
buffer, waits for completion, and verifies the result. After five warmups,
50 trials on the M4 Pro measured:

| measurement | value |
|---|---:|
| CPU submit-to-completion wall median | 131 microseconds |
| wall p95 | 154 microseconds |
| GPU execution median | 7 microseconds |
| initial shader/pipeline setup | 131 milliseconds |

Raw evidence: `docs/encoder_gpu_probe.json`. This is a tiny-dispatch latency
probe, not a SAD kernel, full TVC benchmark, universal latency bound, or
throughput/energy comparison. Cold pipeline setup can be amortized; asynchronous
queues and batching can also change the wall cost substantially.

A synchronous GPU call for each current macroblock would already incur more
latency than the sampled CPU average for the entire block. Useful work would
need larger batches and overlapping execution. Unified memory helps avoid
dedicated-VRAM transfers, but resource preparation and synchronization remain.
The measured wall/GPU difference illustrates why GPU execution time alone is
an insufficient speed metric.

```sh
clang -O2 -fobjc-arc -framework Foundation -framework Metal -o work/encoder_lab/metal_probe tools/tvc_ref/enc/metal_probe.m
work/encoder_lab/metal_probe
```

## GPU design hypothesis

A future prototype could prepare integer SAD costs or motion proposals for
many blocks/candidates together, once a reconstructed reference is available,
and let the CPU perform its current live-context rate-distortion decisions.
The current encoder has left/above predictors, sequential intra reconstruction,
and adaptive coding contexts, so whole-frame decision evaluation is not an
independent map operation. It would take more than replacing a function with
a Metal call. Block-local RDOQ remains branch-heavy and tied to live contexts.
Changing transform analysis from its current double-precision implementation
would require additional rounding and quality validation.

No speedup is estimated for this redesign. Its next gate is finer profiling
of actual encoding-stage motion/candidate work, then an out-of-default-path
batched prototype with wall time including resource preparation and sync,
followed by the complete corpus and level-enforced film gates. GPU code must
preserve the frozen bitstream/decoder contract and keep CPU fallback available.

## Neural Engine assessment

Core ML provides model execution choices including the Neural Engine. The
current encoder has no learned model: its search, transforms, quantization,
adaptive probability estimates and range coder are conventional algorithms.
A Neural Engine compute-unit setting therefore does not accelerate the existing
C encoder directly.

An encoder-only learned model could propose motion candidates or scene cuts
while leaving syntax and final CPU decisions under the existing contract.
That is a new research direction: it needs a model, training data beyond the
four gate clips, deployment/placement measurement, and the same quality/cost
gates. Merely allowing Neural Engine execution in Core ML does not establish
where a model's operations ran or whether it improved throughput. No model was
trained, converted or executed, and no ANE speed/quality/power claim is made.

## Evidence limits and primary sources

This study adds no production codec change, so E-015's accepted corpus and film
gate evidence remains applicable. No new FPGA/decoder cost or Pocket claim is
made. Windows/Linux GPU APIs, native Intel/AMD GPU performance, thermal/power
behavior, full-length-film offload, and actual TVC GPU/ANE speedups are untested.

- [Apple: Metal compute and unified memory](https://developer.apple.com/videos/play/tech-talks/10580/)
- [Apple: performing calculations on a GPU](https://developer.apple.com/documentation/metal/performing-calculations-on-a-gpu)
- [Apple: command-buffer synchronization costs](https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/MTLBestPracticesGuide/CommandBuffers.html)
- [Apple: Core ML compute-unit choices](https://developer.apple.com/documentation/coreml/mlcomputeunits)
- [Apple: Core ML compute-unit selection workflow](https://apple.github.io/coremltools/docs-guides/source/load-and-convert-model.html)

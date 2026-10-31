# Encoder CPU instruction paths

E-023–E-026 re-profile E-022 and evaluate private B-picture workers alongside
VideoToolbox motion and ingestion. The optional worker prototype is 1.29–1.44x
faster with exact streams; production integration remains gated. See
[ENCODER_FOUR_STEP_STUDY.md](ENCODER_FOUR_STEP_STUDY.md).

E-022 further reduces exact RDOQ counting work with cached prefix state. Current
benchmarks and evidence are in [ENCODER_RDOQ_PREFIX.md](ENCODER_RDOQ_PREFIX.md).

E-020 subsequently adds an exact C quarter-pixel search kernel with further
measured gains at 400 and 800 pixels. See [ENCODER_QPEL.md](ENCODER_QPEL.md);
the compiler-profile comparison below is the earlier E-015 measurement.

Mac GPU and Neural Engine opportunities, measured dispatch cost and CPU phase
limits are recorded separately in [ENCODER_ACCELERATORS.md](ENCODER_ACCELERATORS.md).

## Policy and scope

The default pixel kernel is portable C, which the compiler may auto-vectorize.
`-DTVC_SIMD_PIXELS` opts into explicit integer SIMD; `-DTVC_PORTABLE_PIXELS`
overrides it. Selection follows the compiler CPU target, rather than the OS.
No decoder, bitstream, FPGA or firmware files are changed. The kernels use
unaligned 16-byte loads within each complete 16-pixel row, and exact integer
accumulation (maximum 16x16 SAD is 65280).

| CPU/compiler target | Explicit pixel path | OS applicability | Verification here |
|---|---|---|---|
| AArch64 GCC/Clang with NEON | NEON | Apple Silicon macOS; ARM64 Linux; compatible Windows Clang targets | macOS ARM64 native differential tests and corpus gate |
| Native MSVC ARM64 | NEON through `arm64_neon.h` | Windows ARM64 | Header selection follows Microsoft documentation; MSVC build/runtime unverified |
| x86 with SSE2 target, including x86-64 | SSE2 byte SAD | Intel macOS, x86-64 Windows/Linux | macOS x86-64 cross-build and Rosetta correctness; no native Intel/AMD timing |
| Other targets or portable override | C / compiler auto-vectorization | Generic | ARM64 portable differential tests and corpus gate |

The ARM64 explicit-SIMD benchmark versus the prior encoder was mixed:
action QP30 1.0193x, anime QP24 0.9936x, bw_room QP36 1.0080x, from three
alternating repetitions. All paired streams were identical. These small changes
do not establish a consistent benefit; explicit SIMD is therefore opt-in.
See `docs/encoder_speed_simd.json`. Lower-level instructions are not assumed
to improve whole-encoder throughput merely because the kernel is shorter.

No AVX2/AVX-512 path is selected speculatively. Such a path needs native
measurements and dispatch that accounts for CPU and OS support before being
used by a portable distributed binary. Host-specific compiler targets are
not the default. No fast-math flags are introduced.

OS frameworks such as Accelerate/vDSP are deferred: changing the floating-point
transform implementation introduces another rounding/reproducibility gate and
has no measured benefit here. OS video-compression backends are not used: this
work must produce the existing TVC bitstream, and no compatible TVC hardware
backend has been established. Existing segment-level process parallelism in
`encode_film.py` remains the throughput mechanism.

## Reproduction

From the encoder-lab worktree, using the existing GCC/Clang-style build helper:

```sh
python3 tools/encoder_lab.py gate
TVC_ENCODER_CFLAGS='-DTVC_SIMD_PIXELS' python3 tools/encoder_lab.py gate
TVC_ENCODER_CFLAGS='-DTVC_PORTABLE_PIXELS' python3 tools/encoder_lab.py gate
```

Only the encoder receives these extra flags. Decoder/counter builds remain at
their existing `-O2`. Result JSON records the encoder flags. On PowerShell,
set `$env:TVC_ENCODER_CFLAGS` before running Python; a GCC/Clang-compatible
`cc` driver is still required. This is not a claim that the complete existing
toolchain has been validated on Windows or Linux, or that native MSVC can
build all frozen reference files unchanged.

Differential kernel checks (the last command is specific to this Mac):

```sh
cc -O2 -DTVC_SIMD_PIXELS -fsanitize=address,undefined -o work/encoder_lab/check-simd tools/tvc_ref/enc/check_sad.c -lm
work/encoder_lab/check-simd
clang -arch x86_64 -O2 -DTVC_SIMD_PIXELS -o work/encoder_lab/check-sse2 tools/tvc_ref/enc/check_sad.c -lm
work/encoder_lab/check-sse2
```

These include 20000 motion-prediction comparisons plus all-zero/full-contrast
SAD tests with odd strides and all 16 byte alignments. Instruction kernels are
exact; full streams are only required to match within paired builds on the
same architecture, because the encoder's existing floating-point analysis may
make different decisions across CPU/compiler targets.

## Primary references

- [Arm NEON intrinsic reference](https://arm-software.github.io/acle/neon_intrinsics/advsimd.html)
- [Intel intrinsics guide](https://www.intel.com/content/www/us/en/docs/intrinsics-guide/index.html)
- [Microsoft ARM64 NEON header documentation](https://learn.microsoft.com/en-us/cpp/intrinsics/arm64-intrinsics?view=msvc-170)
- [GCC bit-operation builtins](https://gcc.gnu.org/onlinedocs/gcc/Bit-Operation-Builtins.html)
- [Microsoft bit-scan intrinsics](https://learn.microsoft.com/en-us/cpp/intrinsics/bitscanreverse-bitscanreverse64?view=msvc-170)

## Measured compiler fast profile

The build helper retains its default `-O2`. On this Apple Clang 21 ARM64 host,
adding `-O3` without fast math or a host-specific ISA target passed the full
gate and produced identical streams in three repeated paired benchmarks:

| case | O2 median seconds | O3 median seconds | O3 fps | ratio |
|---|---:|---:|---:|---:|
| action QP30 | 2.5644 | 2.4421 | 39.31 | 1.0501x |
| anime_dialog QP24 | 5.7985 | 5.5619 | 17.26 | 1.0425x |
| bw_room QP36 | 2.6120 | 2.4034 | 39.94 | 1.0868x |

Evidence: `docs/encoder_o3.json` and `docs/encoder_speed_o3.json`. These are
native Mac measurements; Windows/Linux compiler and performance gates remain
pending. The default portable-C pixel kernel is used: the compiler already
generates NEON `uabd` instructions in the full ARM64 encoder. Explicit SSE2
full-encoder assembly contains `psadbw`; integer costs contain `clz` on ARM64
and `bsrl` on x86. More explicit intrinsics are therefore not assumed to be
better than generic optimized C.

Build and validate the measured fast profile with:

```sh
TVC_ENCODER_CFLAGS=-O3 python3 tools/encoder_lab.py gate
```

For build only, replace `gate` with `build`. `encode_film.py` uses that built
encoder normally. A later gate without these flags rebuilds at the normal
`-O2`; repeat the environment setting when recording fast-profile runs.
No change to the frozen Makefile or decoder build flags is required.

## Generic integer motion costs

Motion-vector bit estimates use the highest set bit instead of floating-point
`floor(log2())`. GCC/Clang use a guarded `__builtin_clz`; MSVC selects
`_BitScanReverse`; other compilers use a C shift loop. Zero is handled before
the intrinsic. Magnitudes use unsigned arithmetic, including `INT_MIN`.
`-DTVC_PORTABLE_BITS` selects the C implementation for differential checking.
This selects source implementations, rather than forbidding the compiler
from recognizing and optimizing the portable loop itself.

Native ARM64, portable C, and x86-64 under Rosetta pass `check_bits.c`, covering
all values -65535..65535, zero, integer extrema and powers-of-two boundaries.
Address/undefined-behaviour sanitizers pass on the native path. Assembly from
the target compilers contains ARM64 `clz` and x86 `bsrl`. MSVC and native
Windows/Linux execution remain unverified.

```sh
cc -O2 -fsanitize=address,undefined -o work/encoder_lab/check-bits tools/tvc_ref/enc/check_bits.c -lm
work/encoder_lab/check-bits
cc -O2 -DTVC_PORTABLE_BITS -o work/encoder_lab/check-bits-c tools/tvc_ref/enc/check_bits.c -lm
work/encoder_lab/check-bits-c
```

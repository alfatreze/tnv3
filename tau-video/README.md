# tau-video

A portable Rust library for the TNV3/TVC/TAC/TST family. The current
preliminary implementation reads and validates TNV3 headers and declared
index layout, and reads individual index entries through `Read + Seek`. It also provides TVC entropy coding, picture decoding, a preliminary encoder,
and video-only TNV3 muxing. It does not fully validate chunks, subtitles or
index consistency and is not a complete TNV3 checker. Do not use it to certify files.

```sh
cargo test
cargo run --bin tau-video --features cli -- info input.tnv3
```

The default feature set has no dependencies and does not invoke subprocesses.
Enable the opt-in `c-encoder` feature for the safe process-based adapter to the
existing C/Python reference workflow; that adapter uses `serde_json`. Rust
1.85 is the minimum. The library forbids unsafe code. The C adapter requires
external programs and temporary disk space; it is not a native C FFI binding.

For local, non-redistributable reference vectors, set `TAU_VIDEO_VECTOR_DIR`
to the local `tools/tnv3_ref/vectors` folder under `local-media` before running
`cargo test`. Public tests use synthetic bytes. Header-derived lengths and
level limits are checked before use; index access allocates a fixed 16-byte
buffer. The present frame-count limit is 432,000 (four hours at 30 fps).
See `../docs/RUST_ROADMAP.md` and the consumer requirements for remaining work.

## Temporary MP3 sidecars

`make rust` also builds `tau-mp3`, a separate optional command-line adapter
for Tau ν sidecars. It requires FFmpeg with `libmp3lame` on PATH (or set
`TAU_FFMPEG` to its executable). The MP3 command is separate from the library; the C encoder adapter also runs external processes.

```sh
tau-mp3 encode input.wav audio.mp3
tau-mp3 decode audio.mp3 output.wav
```

Encoding produces 48 kHz stereo MP3 at 128 kbit/s; decoding produces 48 kHz
16-bit PCM WAV. Existing output files are not overwritten. MP3 is lossy, so
decoded samples need not equal the input. This adapter does not mux MP3 into
TNV3, generate matching video, or establish audio/video synchronization. MP3
is a temporary sidecar path; TAC and subtitle support remain future work.

The library can decode I/P/B TVC picture payloads and output cropped planar
YUV420 in display order via `tvc::frame::PictureDecoder`. A bounded TVC elementary-stream decoder can also write Y4M via
`tau-video decode-tvc input.tvc output.y4m`. Full TVC/TNV3 level
validation, seeking, a production-quality encoder and a complete container checker remain unimplemented.
The picture decoder has not yet passed exhaustive malformed-input or performance
gates; do not use it as a validator.

With `experimental-rust-encoder`, `tvc::encoder::encode_intra_picture` encodes a single YUV420
frame as an intra-only TVC picture. It uses deterministic integer arithmetic,
DC prediction, 8×8 transforms and one macroblock row per slice. This baseline
is not the optimized C encoder: block-level motion search, general predictive coding, optimized rate-distortion decisions and TNV3 container
output remain future work.

With `--features experimental-rust-encoder`, the baseline CLI supports `tau-video encode-tvc input.y4m output.tvc` at QP 30.
It emits intra pictures and limited whole-frame motion P pictures,
selects a TVC level, and retries pictures at a
higher QP when level picture/rate limits require it. It is not yet optimized for
quality or speed and does not create TNV3 or synchronize an MP3 sidecar.

`tau-video mux-tvc input.tvc output.tnv3` wraps TVC1 in a video-only TNV3
container with CRCs and display timestamps. This does not include an MP3
sidecar or TAC audio, and the muxer does not fully validate video syntax.

`tau-video decode-tnv3 input.tnv3 output.y4m` decodes the video track. Audio
and subtitles are ignored; this is not a full container conformance checker.

The limited motion path searches even-pixel whole-frame translations within
±8 pixels for dimensions divisible by 16. It emits zero-residual P pictures
only if reconstructed YUV mean squared error is at most 4; otherwise it falls
back to intra encoding. This does not replace general block motion search.

The stream encoder also tries zero-motion residual P pictures, including
unaligned display sizes using the reconstructed padded reference. It selects one only when it is smaller than the intra candidate and
its reconstructed total YUV squared error is no more than the intra error plus
one per sample. This is an initial heuristic, not a production quality gate.

The encoder also evaluates residual P pictures using the selected whole-frame
translation. It compares their size and decoded error with an intra candidate;
this adds encoding work and remains a preliminary heuristic.

An experimental block-motion candidate is available with `make
rust-experimental` (Cargo feature `block-motion`). It searches a small even-
pixel window per macroblock and competes with the existing candidates under the
same size/error guard. It is off by default; see
[the experiment record](../docs/RUST_ENCODER_EXPERIMENTS.md).

The optional `b-pictures` feature provides an experimental three-frame stream
mode. Build with `make rust-experimental`, then run `tau-video encode-tvc-b
input.y4m output.tvc`. It reuses the previous anchor and tests a future I or P anchor plus a zero-motion B picture
with either skip or residual coding when quality, size and rate checks pass;
otherwise it falls back to I/P anchor pictures. This mode buffers at most three
frames and is separate from the normal P-picture encoder. Short real-clip tests
showed it can produce substantially larger files than the default; see
[the experiment record](../docs/RUST_ENCODER_EXPERIMENTS.md).

The B-stream experiment compares each two-frame B group against an I/P
alternative using total size and reconstructed YUV error. This guard is
experimental and adds substantial encoding work; it does not guarantee
whole-film rate or perceptual quality improvement.

For inputs that exhaust the rolling video-byte window, `make rust-safe-rate`
builds an opt-in conservative workaround. It caps each picture to the level's
window budget divided by the window length and retries at higher QP. Some
inputs may still fail at QP 51, and quality can change substantially. It is
not look-ahead rate control; see [the issue record](../docs/issues/001-rust-rate-window-starvation.md).
To combine it with the B prototype, build with Cargo features `cli,b-pictures,safe-rate`.

## Reference encoder API

The subprocess-based `tau_video::c_encoder::encode_video` adapter is opt-in;
enable the `c-encoder` Cargo feature to use it. The core library has no default
dependencies or subprocess behavior. Construct `Tools`
with paths to Python 3, `encode_film.py`, FFmpeg and FFprobe, and optionally
override `tvc_enc` and `tnv3` paths. Supply `Options`, a source path, an output
writer, and a segment-progress callback. The synchronous call returns frame
count, level, retries and bounded diagnostics. It creates a video-only TNV3;
applications must encode the MP3 sidecar themselves. The writer is untouched
until encoding and the report succeed; callers must handle partial output if
writing to it fails. External dependencies are not supplied by this crate.

```rust,no_run
use std::{fs::OpenOptions, path::{Path, PathBuf}};
use tau_video::c_encoder::{encode_video, Options, Tools};
let tools = Tools {
    python: PathBuf::from("python3"), script: PathBuf::from("tools/tnv3_ref/encode_film.py"),
    ffmpeg: PathBuf::from("ffmpeg"), ffprobe: PathBuf::from("ffprobe"),
    tvc_encoder: None, tnv3_tool: None,
};
let mut output = OpenOptions::new().write(true).create_new(true).open("video.tnv3")?;
let report = encode_video(&tools, Path::new("source.mp4"), &mut output,
    Options::default(), |done, total| eprintln!("{done}/{total} segments"))
    .map_err(std::io::Error::other)?;
println!("{} frames", report.frames);
# Ok::<(), std::io::Error>(())
```

The pure Rust TVC encoder is parked behind `experimental-rust-encoder`.
The `block-motion`, `b-pictures`, and `safe-rate` features enable it as well.
The experimental API and its compression decisions are not at parity with C.

## Optional Y4M frame grouping

`tau_video::y4m::Y4mFrameGroups` is a streaming helper for explicit 8-bit 4:2:0 Y4M inputs. It writes each frame group directly to a caller-owned sink while preserving the stream header, frame metadata, and payload bytes. Its generic `new` constructor retains 64 KiB line support. `new_c_compatible` opts into the C parser's 511/127-byte line limits and printable ASCII plus LF restriction (tabs, NUL, and controls rejected). It has no subprocess, filesystem, or default encoder integration. Errors poison the reader; discard the current sink. EOF only means the input pipe closed: an orchestrator must wait for FFmpeg and check its exit status before accepting the final output. The deterministic byte-preservation test has no external dependencies. An optional Python parity test runs only when `TAU_TEST_PYTHON` names a Python interpreter; `tests/y4m_python_reference.py` is the reference fixture.

## Experimental C orchestrator

The opt-in Cargo feature `experimental-c-orchestrator` enables `experimental_c_orchestrator::encode_y4m_video`. This vertical slice accepts already-decoded explicit 8-bit 4:2:0 Y4M, writes bounded frame groups, schedules up to six C reference encoder jobs, retries segment boundaries with the same `-P` rate-window context, merges TVC1, and calls `tnv3 mux` plus `tnv3 check` before writing the result. It emits video-only TNV3; source probing, FFmpeg decoding, MP3 encoding, and desktop integration remain outside scope. Frame-rate numerator/denominator must fit TVC's 16-bit fields. C-compatible Y4M header and FRAME lines are capped at 511 and 127 bytes including newline; the Rust Y4M writer enforces 64 MiB per segment including header and metadata. C TVC outputs above 16 MiB are rejected after each child exits. `Options.workspace_limit_bytes` is a projected workspace budget (512 MiB by default), checked before jobs and after child outputs; it is not a hard filesystem quota, so a C child may transiently exceed its reservation before the parent can inspect the completed file. The backend also caps inputs at 432,000 frames and 4,096 segments. If the caller supplies FFmpeg stdout, EOF is not success: keep the TNV3 in private staged storage and publish it only after waiting for FFmpeg and checking its exit status. This backend has only been host-tested on macOS; Windows path handling and runtime behavior are unverified. Generated-media byte parity against the repository's `tools/tnv3_ref/encode_film.py` is opt-in by setting `TAU_TEST_PYTHON`, `TAU_TEST_C_SCRIPT`, `TAU_TEST_FFMPEG`, `TAU_TEST_FFPROBE`, `TAU_TEST_TVC`, and `TAU_TEST_TNV3`; the normal test suite does not need those programs.

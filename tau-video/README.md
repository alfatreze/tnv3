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

Default features have no dependencies. Rust 1.85 is the minimum, matching the
minimum for Tau Omega's Rust 2024 edition. The library forbids unsafe code.
The caller supplies I/O; there is no UI, filesystem assumption or subprocess.

For local, non-redistributable reference vectors, set `TAU_VIDEO_VECTOR_DIR`
to the local `tools/tnv3_ref/vectors` folder under `local-media` before running
`cargo test`. Public tests use synthetic bytes. Header-derived lengths and
level limits are checked before use; index access allocates a fixed 16-byte
buffer. The present frame-count limit is 432,000 (four hours at 30 fps).
See `../docs/RUST_ROADMAP.md` and the consumer requirements for remaining work.

## Temporary MP3 sidecars

`make rust` also builds `tau-mp3`, a separate optional command-line adapter
for Tau ν sidecars. It requires FFmpeg with `libmp3lame` on PATH (or set
`TAU_FFMPEG` to its executable). The Rust library itself does not run external
processes.

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

A preliminary `tvc::encoder::encode_intra_picture` encodes a single YUV420
frame as an intra-only TVC picture. It uses deterministic integer arithmetic,
DC prediction, 8×8 transforms and one macroblock row per slice. This baseline
is not the optimized C encoder: block-level motion search, general predictive coding, optimized rate-distortion decisions and TNV3 container
output remain future work.

The baseline CLI supports `tau-video encode-tvc input.y4m output.tvc` at QP 30.
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

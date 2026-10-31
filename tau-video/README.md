# tau-video

A portable Rust library for the TNV3/TVC/TAC/TST family. The current
preliminary implementation reads and validates TNV3 headers and declared
index layout, and reads individual index entries through `Read + Seek`. It also provides TVC coefficient entropy coding, range coding, picture-header parsing, inverse transforms
intra prediction, motion-compensated inter prediction and picture deblocking. It does
not validate chunks, subtitles or index consistency, and is not yet a TNV3
checker, decoder or encoder. Do not use it to certify files.

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
validation, seeking, the encoder and a complete container checker remain unimplemented.
The picture decoder has not yet passed exhaustive malformed-input or performance
gates; do not use it as a validator.

A preliminary `tvc::encoder::encode_intra_picture` encodes a single YUV420
frame as an intra-only TVC picture. It uses deterministic integer arithmetic,
DC prediction, 8×8 transforms and one macroblock row per slice. This baseline
is not the optimized C encoder: motion prediction, predictive coding, optimized rate-distortion decisions and TNV3 container
output remain future work.

The baseline CLI supports `tau-video encode-tvc input.y4m output.tvc` at QP 30.
It emits intra-only pictures, selects a TVC level, and retries pictures at a
higher QP when level picture/rate limits require it. It is not yet optimized for
quality or speed and does not create TNV3 or synchronize an MP3 sidecar.

`tau-video mux-tvc input.tvc output.tnv3` wraps TVC1 in a video-only TNV3
container with CRCs and display timestamps. This does not include an MP3
sidecar or TAC audio, and the muxer does not fully validate video syntax.

`tau-video decode-tnv3 input.tnv3 output.y4m` decodes the video track. Audio
and subtitles are ignored; this is not a full container conformance checker.

# tau-video

A portable Rust library for the TNV3/TVC/TAC/TST family. The current
preliminary implementation reads and validates TNV3 headers and declared
index layout, and reads individual index entries through `Read + Seek`. It does
not validate chunks, subtitles or index consistency, and is not yet a TNV3
checker, decoder or encoder. Do not use it to certify files.

```sh
cargo test
cargo run --features cli -- info input.tnv3
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

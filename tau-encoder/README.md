# Tau Encoder

A small Tauri + Svelte desktop preview built on the `tau-video` Rust crate. Select an 8-bit YUV420 Y4M input, choose a new `.tnv3` output, and encode a video-only file. The app enables the crate's conservative `safe-rate` feature. Existing files are never overwritten. Encoding runs in a background task; the current crate does not yet expose progress or cancellation, so keep the app open.

The output does not include audio. The separate `tau-mp3` command supplies the temporary MP3 sidecar workflow. The preview can still fail on some inputs and is not a full TNV3 conformance checker. Conversion from common input formats requires a separate tool that produces Y4M. See the crate and project documentation for current limits.

## Development

Install Node.js, Rust and the Tauri v2 platform prerequisites. From this directory run `npm ci`, then `cargo install tauri-cli --version '^2' --locked` and `cargo tauri dev`. To package for the current host use `cargo tauri build`. The path dependency expects this folder next to `tau-video`. Builds for Windows and Linux must be built and validated on their respective platforms; this preview has not yet passed those platform gates.

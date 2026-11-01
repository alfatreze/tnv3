# Tau Encoder

Tauri + Svelte preview using the reusable `tau-video` Rust crate. Select a source video with an audio track, an output folder (including a mounted card), and a base name. The app creates matching `.tnv3` and `.mp3` files and remembers the last selected folder. Existing files are never overwritten. Video is resized to fit 400×360, converted to 24 fps and encoded with conservative rate control; MP3 is 48 kHz stereo at 128 kbit/s. Some inputs may fail or lose quality due to current rate control. Audio and video synchronisation has not yet been verified on Pocket hardware. Progress and cancellation are not yet available.

FFmpeg is invoked only by the desktop app; the core crate remains pure Rust. Packaged builds can include a separate FFmpeg executable under `ffmpeg/`; development falls back to `TAU_FFMPEG`, a Homebrew installation on macOS, or `ffmpeg` on PATH. The source must contain a supported audio stream; silent-video generation is not yet implemented. MPEG-4, H.264, HEVC, WebM and other inputs depend on the codecs present in the selected FFmpeg build.

## Development

Install Node.js, Rust and the Tauri v2 platform prerequisites. Run `npm ci`, then `cargo install tauri-cli --version '^2' --locked` and `cargo tauri dev`. To package for the current host use `cargo tauri build`. The path dependency expects this folder next to `tau-video`. The platform builds must be validated on their respective platforms; this preview is not a production release.

For a self-contained macOS bundle, run `scripts/build-ffmpeg-macos.sh` and then `cargo tauri build --config src-tauri/tauri.bundle.conf.json`. This builds LGPL FFmpeg 6.1.1 and a dynamically linked LGPL LAME 4.0 and includes their source archives, licenses and build instructions with the app. The generated helpers are ignored by Git. Equivalent bundled helpers and packaging for Windows and Linux are still outstanding.

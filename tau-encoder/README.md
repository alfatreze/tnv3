# Tau Encoder

A Tauri + Svelte desktop preview using the reusable `tau-video` Rust crate. Choose a video source, destination folder and encoder. The Rust encoder defaults to suffix `_R`, and the C encoder to `_C`; edit the suffix to distinguish your files. Each encode produces matching `.tnv3` and `.mp3` files and remembers the output folder. Existing files are never overwritten.

The C option runs the same segmented `tools/tnv3_ref/encode_film.py` and C reference encoder used in the test workflow, with video-only output and the app's 400×360 maximum, 24 fps scaling. Python 3 must be installed and available as `python3` on macOS/Linux or `python` on Windows; set `TAU_PYTHON` to an interpreter if needed. The C path needs enough temporary storage for decoded segments. The existing C helper's usual defaults and TAC mode are unchanged outside the app.

Local preview opens a TNV3 and its matching MP3 if present. It decodes the video on the host, supports play/pause and drops late video frames against the audio clock. It is for visual checks, not a Pocket performance emulator or a full conformance checker. Seeking and replay without reopening are not implemented; the preview has not been validated on all platforms.

Both encoders currently require a source audio track. Output is 24 fps and 48 kHz stereo MP3; the Rust encoder uses conservative rate control and may fail or reduce quality. The app shows an updating video estimate for Rust and segment progress for C; audio and saving are separate. Cancellation is not yet available. Pocket A/V synchronisation remains to be validated.

## Development

Install Node.js, Rust and the Tauri v2 platform prerequisites. From this directory run `npm ci`, then `cargo install tauri-cli --version '^2' --locked`, compile the C tools with `make` in the repository root, and run `cargo tauri dev`. FFmpeg and FFprobe are used by the app; set `TAU_FFMPEG` or install them on PATH if a bundled copy is absent.

For a self-contained macOS helper bundle, run `scripts/build-ffmpeg-macos.sh`, then `scripts/build-codec-tools.sh`, then `cargo tauri build --config src-tauri/tauri.bundle.conf.json`. This includes the C tools and LGPL FFmpeg 6.1.1 dynamically linked to LGPL LAME 4.0, with corresponding sources, licenses and build instructions. Python remains a separate dependency. Equivalent bundled helpers and platform validation on Windows and Linux remain outstanding.

When FFmpeg reports an error during video or audio ingest, the app displays a bounded diagnostic warning even if FFmpeg exits successfully. A successful encode or absence of warnings does not prove the source has no silent corruption.

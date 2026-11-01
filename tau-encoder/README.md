# Tau Encoder

A Tauri + Svelte desktop preview using the reusable `tau-video` Rust crate. Choose a video source, destination folder and encoder. The Rust encoder defaults to suffix `_R`, and the C encoder to `_C`; edit the suffix to distinguish your files. Each encode produces matching `.tnv3` and `.mp3` files and remembers the output folder. Existing files are never overwritten.

The C option runs the same segmented `tools/tnv3_ref/encode_film.py` and C reference encoder used in the test workflow, with video-only output and the app's 400×360 maximum, 24 fps scaling. Python 3 must be installed and available as `python3` on macOS/Linux or `python` on Windows; set `TAU_PYTHON` to an interpreter if needed. The C path needs enough temporary storage for decoded segments. The existing C helper's usual defaults and TAC mode are unchanged outside the app.

Local preview opens a TNV3 and its matching MP3 if present. It decodes the video on the host, supports play/pause and drops late video frames against the audio clock. It is for visual checks, not a Pocket performance emulator or a full conformance checker. Seeking and replay without reopening are not implemented; the preview has not been validated on all platforms.

Both encoders currently require a source audio track. Output is 24 fps and 48 kHz stereo MP3; the Rust encoder uses conservative rate control and may fail or reduce quality. The app shows an updating video estimate for Rust and segment progress for C; audio and saving are separate. Cancellation is not yet available. Pocket A/V synchronisation remains to be validated.

## Dependency settings

Open **Dependency settings** to browse to or enter full paths for FFmpeg, FFprobe, Python 3, `encode_film.py`, `tvc_enc` and `tnv3` (use the `.exe` programs on Windows). The C encoder needs all six; Rust encoding uses FFmpeg. Selected paths override automatic discovery and are remembered on this computer. Clear a field to return to the bundled, environment or default tool. If FFprobe is blank, the app looks beside the selected or automatically discovered FFmpeg. A selected file must exist; executable permissions and compatibility with the current platform are still required. FFmpeg must support the source formats and `libmp3lame`.

These controls do not themselves bundle dependencies or establish Windows/Linux runtime compatibility. The Python script and C programs should come from a matching Tau codecs build. Test a short encode after configuring them.

## Advanced Pocket experiments

Both encoder choices share these optional ingest/audio controls. Defaults remain unchanged. For a meaningful comparison, use the same source and encoder, change one option at a time, and choose a distinct suffix for each output pair. Test the full video and listen for synchronization on Pocket.

- **Round frame timestamps down** may avoid repeated frames with some source timestamps. Generated tests also showed a missing final frame and a one-frame shift with variable-rate input.
- **Pass the final frame with round-down** can restore a missing frame but produced an additional frame in other cases. It only applies when round-down is enabled.
- **Test MP3 delay compensation** removes the first 1,105 samples (about 23 ms) of source audio before MP3 encoding. This permanently discards that audio and may make sync better or worse. The amount comes from an FFmpeg host model, not a Helix or Pocket measurement. Do not assume it corrects hardware synchronization.

Source decoding diagnostics remain enabled for both encoders; they cannot detect every corrupt source. These options do not repair the unresolved Pocket frame freeze, change the player, or make the Rust and C encoder implementations equivalent. See `docs/issues/003-rust-encoder-player-throughput.md`, `005-hevc-input-ingest.md`, and `006-mp3-gapless-timing.md` in the repository.

## Development

Install Node.js, Rust and the Tauri v2 platform prerequisites. From this directory run `npm ci`, then `cargo install tauri-cli --version '^2' --locked`, compile the C tools with `make` in the repository root, and run `cargo tauri dev`. FFmpeg and FFprobe are used by the app; set `TAU_FFMPEG` or install them on PATH if a bundled copy is absent.

For a self-contained macOS helper bundle, run `scripts/build-ffmpeg-macos.sh`, then `scripts/build-codec-tools.sh`, then `cargo tauri build --config src-tauri/tauri.bundle.conf.json`. This includes the C tools and LGPL FFmpeg 6.1.1 dynamically linked to LGPL LAME 4.0, with corresponding sources, licenses and build instructions. Python remains a separate dependency. Equivalent bundled helpers and platform validation on Windows and Linux remain outstanding.

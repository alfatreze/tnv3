# Tau Encoder

A Tauri + Svelte desktop preview using the reusable `tau-video` Rust crate. Choose a video source and destination folder. The C reference encoder is the default, with suffix `_C`; edit the suffix to distinguish your files. A build with `--features experimental-rust-encoder` exposes the parked Rust encoder with suffix `_R`. Each encode produces matching `.tnv3` and `.mp3` files and remembers the output folder. Existing files are never overwritten.

When the optional Rust encoder is enabled, both options share ingest and sidecar settings, but their compression decisions are not yet equivalent. C is the more developed reference encoder; Rust remains preliminary. See `docs/RUST_C_ENCODER_PARITY.md` in the repository.

The C option defaults to the segmented Python + C workflow (`tools/tnv3_ref/encode_film.py`), with video-only output and the app's 400×360 maximum, 24 fps scaling. Python 3 must be installed and available as `python3` on macOS/Linux or `python` on Windows; set `TAU_PYTHON` to an interpreter if needed. An experimental Python-free Rust orchestration option runs the same C video encoder and does not change audio encoding. It avoids the Python dependency and tracks a projected temporary workspace budget (default 512 MiB); long films may need a higher user-set budget. Actual disk use may vary from the projection. Both workflows require enough temporary disk space for their processing. The separate experimental Rust encoder mode remains unchanged and does not use this workflow selector.

Local preview opens a TNV3 and its matching MP3 if present. It decodes the video on the host, supports play/pause and drops late video frames against the audio clock. It is for visual checks, not a Pocket performance emulator or a full conformance checker. Seeking and replay without reopening are not implemented; the preview has not been validated on all platforms.

The app currently requires a source audio track. Output is 24 fps and 48 kHz stereo MP3; the Rust encoder uses conservative rate control and may fail or reduce quality. The Python workflow reports segment progress; the native C workflow reports completed segments and updates its video-time estimate. Audio and saving are separate. Cancellation is not yet available. Pocket A/V synchronisation remains to be validated.

## Dependency settings

Open **Settings** to browse to or enter full paths for FFmpeg, FFprobe, Python 3, `encode_film.py`, `tvc_enc` and `tnv3` (use the `.exe` programs on Windows). The Python workflow uses these tools; the native C workflow uses FFmpeg and the two C tools without Python or the script. Selected paths override automatic discovery and are remembered on this computer. Clear a field to return to the bundled, environment or default tool. If FFprobe is blank, the app looks beside the selected or automatically discovered FFmpeg. A selected file must exist; executable permissions and compatibility with the current platform are still required. FFmpeg must support the source formats and `libmp3lame`.

These controls do not themselves bundle dependencies or establish Windows/Linux runtime compatibility. The Python script and C programs should come from a matching Tau codecs build. Test a short encode after configuring them.

## Advanced Pocket experiments

The C encoder and the optional experimental Rust encoder share these optional ingest/audio controls. Defaults remain unchanged. For a meaningful comparison, use the same source and encoder, change one option at a time, and choose a distinct suffix for each output pair. Test the full video and listen for synchronization on Pocket.

- **Round frame timestamps down** may avoid repeated frames with some source timestamps. Generated tests also showed a missing final frame and a one-frame shift with variable-rate input.
- **Pass the final frame with round-down** can restore a missing frame but produced an additional frame in other cases. It only applies when round-down is enabled.
- **Test MP3 delay compensation** removes the first 1,105 samples (about 23 ms) of source audio before MP3 encoding. This permanently discards that audio and may make sync better or worse. The amount comes from an FFmpeg host model, not a Helix or Pocket measurement. Do not assume it corrects hardware synchronization.

Source decoding diagnostics remain enabled; they cannot detect every corrupt source. These options do not repair the unresolved Pocket frame freeze, change the player, or make the Rust and C encoder implementations equivalent. See `docs/issues/003-rust-encoder-player-throughput.md`, `005-hevc-input-ingest.md`, and `006-mp3-gapless-timing.md` in the repository.

## Development

Install Node.js, Rust and the Tauri v2 platform prerequisites. From this directory run `npm ci`, then `cargo install tauri-cli --version '^2' --locked`, compile the C tools with `make` in the repository root, and run `cargo tauri dev`. FFmpeg and FFprobe are used by the app; set `TAU_FFMPEG` or install them on PATH if a bundled copy is absent.

For a self-contained macOS helper bundle, run `scripts/build-ffmpeg-macos.sh`, then `scripts/build-codec-tools.sh`, then `cargo tauri build --config src-tauri/tauri.bundle.conf.json`. This includes the C tools and LGPL FFmpeg 6.1.1 dynamically linked to LGPL LAME 4.0, with corresponding sources, licenses and build instructions. Python remains a separate dependency. Equivalent bundled helpers and platform validation on Windows and Linux remain outstanding.

## App versioning

The desktop app follows semantic versioning independently of the codec format and Rust crate. Version 0.2.1 is a preview: increment the patch for compatible fixes, the minor for new functionality or breaking changes before 1.0, and the major for breaking changes after 1.0. Prereleases use a SemVer suffix such as `-alpha.1`. Update the npm, Tauri, Cargo and lockfile versions together; `npm run check-version` checks their agreement in CI. Name distributed builds `Tau-Encoder-<version>-<platform>` and document changes in the changelog.

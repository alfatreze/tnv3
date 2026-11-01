# Tau video CLI prototype

This standalone crate sits beside `tau-video` and does not modify the Tau codecs checkout.

On macOS, the default command writes a TNV3 video and a matching MP3 sidecar:

```sh
cargo run --release -- encode input.mp4 output.tnv3
```

The checkout is discovered from the current directory or `TAU_CODECS_ROOT`. Python 3, FFmpeg, and FFprobe are discovered on `PATH`. To supply tools directly, use `--codec-root PATH`, `--script PATH`, `--python PATH|NAME`, `--ffmpeg PATH|NAME`, `--ffprobe PATH|NAME`, `--tvc-encoder PATH|NAME`, and `--tnv3-tool PATH|NAME`. Bare executable names are resolved on `PATH`; Unix lookup skips files without execute permission. Paths work for bundled tools outside the checkout. The script and C helpers can be omitted when the checkout supplies them. Use `--video-only` to omit MP3. `--qp` defaults to 30 and `--jobs` to 1.

The MP3 uses the current Tau encoder settings: padded audio limited to the encoded frame duration, 48 kHz stereo, 128 kbit/s, and stripped metadata. No independent MP3 analysis or transformation is performed.

Both outputs refuse to replace existing files. On Linux and macOS, the CLI first stages data in a private sibling directory and prefers directory-relative no-replace renames. If the filesystem reports that this operation is unsupported (`ENOTSUP`/`EOPNOTSUPP`), it falls back to creating each final name exclusively (`create_new`) and copying/syncing the staged bytes. It never falls back to ordinary rename and never unlinks final output paths on error. The copied final name is visible while bytes are being written; a copy, sync, process, or power failure may leave a partial file. If MP3 publication fails after TNV3 is visible, the TNV3 remains and the CLI reports the incomplete pair. Inspect named partial outputs before retrying. Pair publication is not atomic. The output directory must be trusted and not group/world-writable.

The staged exFAT fallback was reviewed and tested from an isolated source copy against the TauV exFAT card using generated media. The fallback is integrated into the CLI; it remains an experimental prototype, not a release-qualified cross-platform path. This host filesystem evidence does not verify Pocket playback or A/V synchronization.

On Windows, outputs are copied directly to final paths reserved with `create_new`. They may be visible while incomplete; pair publication is not atomic. If a later MP3 open, copy, or flush fails after the TNV3 has been published, the error identifies the remaining TNV3 path. Final outputs are never unlinked on error, and existing paths are not overwritten. This MVP makes no ACL, crash-consistency, or concurrent-directory protection guarantee. Inspect partial outputs before retrying. Windows runtime behavior has not been verified here.

The generated `smoke.py` and `pocket_sidecar_smoke.py` helpers remain optional. The latter verifies generated TNV3 plus MP3 output, format properties, and no-overwrite behavior on the host; it does not test Pocket hardware.

## CI artifacts

At commit `1e5bd30`, CLI workflow run `41342981023` and desktop workflow run
`41342981138` succeeded on macOS, Windows, and Linux and uploaded executable-only
artifacts. They are not installers and do not bundle dependencies. These CI
results confirm build/test jobs only; Windows runtime remains unverified. A
separate macOS-hosted exFAT card run is documented in issue 008; it does not
establish Windows FAT/exFAT behavior.

# Tau video CLI prototype

This standalone crate sits beside `tau-video` and does not modify the Tau codecs checkout.

On macOS, the default command writes a TNV3 video and a matching MP3 sidecar:

```sh
cargo run --release -- encode input.mp4 output.tnv3
```

The checkout is discovered from the current directory or `TAU_CODECS_ROOT`. Python 3, FFmpeg, and FFprobe are discovered on `PATH`. To supply tools directly, use `--codec-root PATH`, `--script PATH`, `--python PATH|NAME`, `--ffmpeg PATH|NAME`, `--ffprobe PATH|NAME`, `--tvc-encoder PATH|NAME`, and `--tnv3-tool PATH|NAME`. Bare executable names are resolved on `PATH`; Unix lookup skips files without execute permission. Paths work for bundled tools outside the checkout. The script and C helpers can be omitted when the checkout supplies them. Use `--video-only` to omit MP3. `--qp` defaults to 30 and `--jobs` to 1.

The MP3 uses the current Tau encoder settings: padded audio limited to the encoded frame duration, 48 kHz stereo, 128 kbit/s, and stripped metadata. No independent MP3 analysis or transformation is performed.

`--rust-c-orchestrator` opts into a macOS-host-tested experimental path. FFmpeg decodes video to Y4M stdout using the desktop 400x360 aspect-fit, 24 fps profile; Rust streams that pipe into the C segment encoder with the Python workflow's default 480-frame segment size and infers the smallest fitting level from the actual Y4M header. A 128 MiB per-segment Y4M cap fits 480 full-size desktop frames including headers and metadata. All raw Y4M segment files remain on disk until encoding and muxing finish; with the default 512 MiB projected workspace budget, expect roughly one to two minutes at full desktop output size, depending on the retained C segment bytes and job count. `--workspace-mib N` raises that projected cap for longer videos; it is not a hard filesystem quota. FFmpeg stderr capture is capped at 4 KiB. On Y4M errors the CLI kills and reaps the direct FFmpeg child; it does not manage a process group, so descendants may outlive the child. If a descendant keeps the inherited stderr pipe open after FFmpeg exits, the CLI waits at most two seconds, returns an error without publication, and detaches the diagnostics reader until that pipe closes. The video-only TNV3 remains in the private stage until FFmpeg exits successfully; the MP3 sidecar is then encoded and the pair is published. A nonzero decoder exit or missing audio leaves neither final name published. Generated 121- and 481-frame fixtures matched the Python C workflow byte-for-byte for TNV3 and MP3; the 481-frame case used two segments on each path. Windows runtime behavior is unverified.

Run the reproducible 481-frame two-segment parity smoke with explicit tools:

```sh
python3 tau-video-cli/rust_c_parity_smoke.py \
  --codec-root "$TAU_CODECS_ROOT" \
  --ffmpeg "$TAU_FFMPEG" \
  --ffprobe "$TAU_FFPROBE"
```

It generates media in a temporary directory, checks both TNV3 files, compares TNV3 and MP3 bytes, reports fallback warnings, and removes the generated source and outputs when it exits.

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

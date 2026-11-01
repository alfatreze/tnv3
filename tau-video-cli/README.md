# `tau-codecs-cli` integration candidate

This standalone crate is staged beside a fresh copy of `tau-video`. Cargo uses
that local path dependency and enables its existing `c-encoder` feature
explicitly. No files in the Tau codecs checkout are modified.

The CLI invokes `tau_video::c_encoder::encode_video` to write TNV3. It takes
explicit Python, script, FFmpeg, FFprobe, TVC encoder and TNV3 tool paths, plus
QP and segment job count. The default remains video-only. The opt-in
`--profile pocket-sidecar` also encodes a matching MP3 using tau-encoder's
current defaults: `apad`, output limited to `frames / 24` seconds, 48 kHz,
stereo, `libmp3lame` at 128 kbit/s, with metadata removed. The MP3 name uses
the same base name and `.mp3` extension as the requested TNV3 path.

On Linux and macOS, it stages both pair assets in a private sibling directory.
Each final path uses atomic no-replace rename, preserving pre-existing files
and symlinks. Pair publication itself is not atomic: TNV3 is briefly visible
before MP3. If MP3 publication fails, rollback attempts to remove the just
published TNV3. This is best effort; if rollback fails, TNV3 may remain. A
same-user concurrent process can race rollback's name-based unlink, so the
directory should not be modified concurrently during pair publication. The
output parent must be trusted and not group/world-writable, including
directories with the sticky bit. Publication is anchored to the opened parent
directory; if another actor renames that directory, publication may occur in
the pinned directory at its new pathname. The implementation does not prevent
or detect that pathname change. Unsupported atomic rename behavior fails
closed. Windows and other platforms explicitly report unsupported.
FAT/exFAT behavior has not been verified and is not promised.

FFmpeg opens the staged MP3 through the output directory's pathname. If the
parent directory is renamed while encoding is in progress, FFmpeg may fail to
open that staging path; in that case no final pair is published.

## Build and unit tests

From this directory:

```sh
cargo fmt --check
cargo test --offline
cargo build --offline
```

The `c-encoder` feature is enabled in this crate's dependency declaration; the
generated smoke additionally needs the C-reference tools in a Tau codecs
checkout:

```sh
python3 smoke.py --codec-root "/path/to/Tau codecs"
```

The pair-specific integration smoke also checks the TNV3 reference checker,
decoded frame count, MP3 stream properties, no-audio cleanup, and preservation
of existing output paths:

```sh
python3 pocket_sidecar_smoke.py --codec-root "/path/to/Tau codecs" \
  --codec-tools "/path/to/bundled/codec-tools" \
  --tvc-decoder "/path/to/tvc_dec" \
  --ffmpeg "/path/to/bundled/ffmpeg" --ffprobe "/path/to/bundled/ffprobe"
```

When no `--tvc-decoder` is supplied, the smoke uses
`<codec-root>/tools/tvc_ref/tvc_dec`; app bundles that omit the decoder can
pass the local reference decoder explicitly.

The smoke script builds a generated 24-frame input, runs the C container
checker, confirms overwrite refusal preserves bytes, and uses a temporary Cargo
target directory. It is a helper-level host smoke test, not a Rust desktop
integration, warning-path test, or Pocket hardware test. Strict FFmpeg warning
checks belong to the earlier helper regression fixture.

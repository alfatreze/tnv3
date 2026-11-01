# `tau-codecs-cli` integration candidate

This standalone crate is staged beside a fresh copy of `tau-video`. Cargo uses
that local path dependency and enables its existing `c-encoder` feature
explicitly. No files in the Tau codecs checkout are modified.

The CLI invokes `tau_video::c_encoder::encode_video` to write video-only TNV3.
It takes explicit Python, script, FFmpeg, FFprobe, TVC encoder and TNV3 tool
paths, plus QP and segment job count. It does not create an MP3 sidecar or an
audio/video pair.

On Linux and macOS, it stages the output in a private sibling directory and
publishes via atomic no-replace rename. Existing files and symlinks are
preserved. The output parent must be trusted and not group/world-writable,
including directories with the sticky bit. Publication is anchored to the
opened parent directory; if another actor renames that directory, publication
may occur in the pinned directory at its new pathname. The implementation does
not prevent or detect that pathname change. Unsupported atomic rename behavior
fails closed. Windows and other platforms explicitly report unsupported.
FAT/exFAT behavior has not been verified and is not promised.

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

The smoke script builds a generated 24-frame input, runs the C container
checker, confirms overwrite refusal preserves bytes, and uses a temporary Cargo
target directory. It is a helper-level host smoke test, not a Rust desktop
integration, warning-path test, or Pocket hardware test. Strict FFmpeg warning
checks belong to the earlier helper regression fixture.

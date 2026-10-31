# TNV3 codec tools — 0.1.0-alpha.1 candidate

TNV3 is a host-side toolkit for the Tau TVC video, TAC audio and TNV3 container
formats. This initial source release contains the validated serial C encoder,
reference decoders, audio encoder and container tools. The planned pure-Rust
`tau-video` crate is not included yet.

On a POSIX system with a C compiler, make and Python 3:

```sh
make
make test
python3 tools/tnv3_ref/encode_film.py SOURCE OUTPUT.tnv3 --width 800
```

The film helper additionally needs FFmpeg and FFprobe. It defaults to the
serial TVC encoder and segment-level parallelism. A separately built,
experimental encoder can parallelize **existing** slices:

```sh
make experimental
python3 tools/tnv3_ref/encode_film.py SOURCE OUTPUT.tnv3 --width 800 \
  --experimental-parallel-slices --cpu-budget 4
```

This option does not add slices to a stream; level 3 already requires at least
two. The budget limits concurrent TVC encoder slots, not FFmpeg, audio or OS
threads. `--encoder-threads 1` forces the serial slice fallback. The experimental
path has bounded race and failure testing but is not production-qualified.

The four format files in `docs/` are snapshots. Tau V retains normative format
authority until that is intentionally transferred. `LICENSE`, `NOTICE.md` and
`SOURCE_MANIFEST.json` preserve upstream provenance. The standalone smoke test
uses generated content; existing vectors containing third-party film/audio are
not redistributed pending provenance review.

Validated on macOS ARM64; the POSIX code is intended for Linux but has not had
native Linux testing in this release preparation. The Windows worker backend has
been exercised only through a model, not a native Windows build or runtime.
The reference encoder holds its complete input in memory. See `VALIDATION.md`
and the encoder reports for measured results and remaining gates. This alpha is
not a stable library API or a completed Rust crate.

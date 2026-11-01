# TNV3 codec tools — 0.1.0-alpha.2 candidate

TNV3 is a host-side toolkit for the Tau TVC video, TAC audio and TNV3 container
formats. This initial source release contains the validated serial C encoder,
reference decoders, audio encoder, fixed-point audio decoder model and container tools. The pure-Rust `tau-video` crate is under development; it currently inspects
TNV3 headers and index entries and decodes TVC picture payloads. The TVC elementary-stream decoder can write Y4M; TNV3 decoding, complete
validation and the encoder are pending.

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

## Research highlights

The project explored MPEG-4 playback, a constrained AV1 decoder, and a custom
codec family. These are separate tracks; historical host results do not imply
that every format runs on Pocket hardware.

- **MPEG-4 and AV1:** In a nine-clip host study, full libaom AV1 averaged 73%
  smaller files than tuned MPEG-4 at comparable VMAF; the leanest tested AV1
  configuration averaged 66% smaller. These are study-specific estimates, not
  measurements of TVC or of an AV1 hardware implementation.
- **TVC encoder:** On three 400-pixel, 96-frame host cases, the optimized serial
  encoder ran 1.72–2.19× as fast as the original. This comparison includes
  compiler changes and uses the same nominal QP, not equal-quality settings. A
  separate four-clip quality gate measured a mean 1.52% lower BD-rate.
- **Parallel work:** Parallelizing slices already required by level 3 reached
  1.82× serial throughput in one 800-pixel action case with the same slice
  layout. Creating extra slices for speed increased mean BD-rate by 9.26% in a
  separate test, so the worker remains opt-in and experimental.
- **Apple acceleration:** VideoToolbox motion proposals and hardware-assisted
  input did not improve throughput in the tested workflows. GPU and Neural
  Engine performance has not been established, and VideoToolbox does not
  implement the custom TVC format.

See [the research index](docs/RESEARCH_INDEX.md),
[codec comparison](docs/CODEC_PHASE0B.md),
[encoder results](docs/ENCODER_TOTAL_RESULTS.md), and
[parallel experiments](docs/ENCODER_PARALLEL_HYPOTHESES.md) for methods and
limits.

The four format files in `docs/` are snapshots. Tau V retains normative format
authority until that is intentionally transferred. `LICENSE`, `NOTICE.md` and
`SOURCE_MANIFEST.json` preserve upstream provenance. The standalone smoke test uses generated content; media-derived clips and
vectors are not included in public packages. Local samples can be kept in
`local-media/`, which Git ignores. See `docs/RESEARCH_INDEX.md` for the format research
and historical measurements.

The standalone synthetic smoke test passes on macOS ARM64 and GitHub-hosted
Ubuntu and macOS runners. Full native Linux corpus and performance testing has
not yet been run. The Windows worker backend has
been exercised only through a model, not a native Windows build or runtime.
The reference encoder holds its complete input in memory. See `VALIDATION.md`
and the encoder reports for measured results and remaining gates. This alpha is
not a stable library API or a completed Rust crate.

## Rust library and CLI (in development)

`make rust` builds the library and the `tau-video info FILE` command;
`make rust-test` runs its tests. The current reader validates header and
declared layout only, so it cannot certify files. See [tau-video/README.md](tau-video/README.md).
A future cross-platform GUI can use this UI-independent library.

For temporary Tau ν MP3 sidecars, `make rust` also builds `tau-mp3`, an
FFmpeg-backed encoder/decoder. See [the crate README](tau-video/README.md)
for usage and limitations. The custom TAC codec and subtitles remain future work.

Decode a TVC elementary stream with `tau-video decode-tvc input.tvc output.y4m`
after `make rust`. This decoder is not yet a complete conformance checker.

A preliminary encoder with limited whole-frame-motion P pictures is available as `tau-video encode-tvc
input.y4m output.tvc`. It is a functional baseline, not yet comparable to the
optimized C encoder for compression efficiency or speed.

`tau-video mux-tvc input.tvc output.tnv3` creates a video-only TNV3 file;
the temporary MP3 audio remains a separate sidecar.

`tau-video decode-tnv3 input.tnv3 output.y4m` decodes the video track; it
does not play or extract audio or subtitles.

## Desktop encoder preview

[`tau-encoder/`](tau-encoder/README.md) contains a Tauri and Svelte desktop
preview that uses the Rust crate to encode Y4M to video-only TNV3. The first
macOS host build and a generated-frame GUI encode were tested; other platform
builds remain to be validated. MP3 remains a separate sidecar tool.

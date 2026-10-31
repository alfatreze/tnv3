# Changelog

## 0.1.0-alpha.1 (in development)

- Add TNV3 v0 draft 1.4 header and index-layout inspection and an `info` CLI.
- Add bounded header validation and synthetic malformed-input tests.
- A full checker, decoder, encoder, player and subtitle support remain pending.
- Add an optional FFmpeg-backed MP3 sidecar encode/decode CLI (temporary).
- Add draft 5 TVC slice header parsing and range coding; cross-check the range encoder against a C reference fixture.
- Add TVC inverse transforms and intra prediction, tested against synthetic C reference fixtures.
- Add half-pixel, quarter-pixel luma and eighth-pixel chroma inter prediction, with C-reference fixtures.
- Add complete TVC luma/chroma picture deblocking, tested against synthetic C-reference fixtures.
- Add draft 5 TVC coefficient block encoding and decoding, checked against a C-reference bitstream.
- Add TVC macroblock motion-vector prediction and signed Exp-Golomb coding, checked against C-reference fixtures.
- Decode intra-picture macroblock syntax within a slice; checked against a synthetic C-reference fixture.
- Reconstruct standalone sliced or unsliced TVC intra pictures into YUV420; compared against the C decoder on local vectors.
- Decode P/B macroblocks and pictures, retain anchor references and return frames in display order.
- Add TVC elementary-stream decoding to Y4M as a library function and CLI command.
- Add a deterministic baseline single-picture intra encoder, cross-checked with the C decoder.
- Add baseline intra-only Y4M-to-TVC stream encoding with level selection and size retries.
- Mux TVC1 streams into video-only TNV3 containers with display order and chunk CRCs.
- Decode the video track of TNV3 files to Y4M; audio and subtitles are ignored.
- Add bounded zero-motion P pictures for near-static frames and an exact separable forward transform.
- Add bounded whole-frame integer translation for zero-residual P pictures.
- Add zero-motion residual P-picture candidates with size and reconstruction-error selection.
- Evaluate whole-frame motion plus residual P-picture candidates.
- Add opt-in experimental block-motion P-picture search.
- Add a sampled precheck to the opt-in block-motion experiment; default remains unchanged.

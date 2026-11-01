# 005: Source ingest can repeat frames or conceal decoder errors

**Status:** open; reproducible synthetic **host** tests, 2026-11-01. No
conclusion about the user's specific HEVC file or Pocket freeze.

The earlier damaged 10-bit HEVC episode had zero-filled holes, a broken seek
index and silent corruption, including corruption missed by a detector; see
`docs/CODEC_PHASE0B.md`. That is evidence about that file, not all HEVC.

The desktop app's bundled FFmpeg 6.1.1 is used to decode the source, scale
it and run `fps=24` before feeding Y4M to the Rust or C encoder. On generated
24 fps 8-bit HEVC, 10-bit HEVC and 8-bit H.264 MKV files with AAC audio, the
current FFmpeg 6.1.1 Y4M output had 72 frames but 24 adjacent repeats. The
FFmpeg 9.0.2 system build output 72 with zero adjacent repeats. Before the
filter, the two decoders' native pictures matched on all generated cases.
This is a pipeline/version/timestamp finding, not evidence that HEVC decode
is generally wrong. Other generated rates are in `hevc_ingest_probe_2026-11-01.json`.

On a generated damaged HEVC sample, a 64-byte mutation changed 36 decoded
frames without a decoder diagnostic; both default and `-xerror` returned 0.
A different mutation caused explicit HEVC errors yet FFmpeg 6.1.1 returned 0
with and without `-xerror`. The Rust CLI video path and Python/C helper produced valid video-only
TNV3 in a separate test of the reported-error mutation. Rust
discards FFmpeg stderr; the C wrapper records it temporarily but does not
surface it after an otherwise successful encode. A valid TNV3 says nothing
about whether source frames were decoded correctly.

**Candidate, not shipped:** `fps=24:round=down` eliminated the unwanted
repeats in the 24 fps synthetic cases and produced frame-identical Y4M to
FFmpeg 9's default in the 11 generated cases (12–60 fps, H.264 and HEVC,
including one source with a 0.25 s video delay). This is not proof of general
correctness. Resetting video PTS to zero removed that delay; do not apply it
without an A/V timing design. The app also requests 8-bit YUV and has no
explicit HDR tone mapping; HDR handling is an untested limitation, not a
measured cause of this issue.

**Next:** reproduce against real clean and damaged HEVC files and generated
variable frame rates, nonzero positive/negative track offsets and audio-sync
markers; compare to independent decoding and inspect frames visually. Decide
a rate-conversion policy, retain/report bounded decoder diagnostics in both
paths, and test recovery/error behavior before changing defaults. Error
logging, strict decoding or `-xerror` cannot detect every silent corruption.
Run `tools/codec_study/ingest_probe.py` with a libx264/libx265-capable
generator and the bundled FFmpeg; aggregate evidence is in the JSON file.

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

## Follow-up: bounded warnings and timestamp markers

**Host, 2026-11-01:** the desktop app now drains FFmpeg/Python stderr in a
separate thread, retains at most 4,096 bytes per process and shows a warning
when a successful video or audio step reports an error. Failures include the
bounded diagnostics. It does not abort or delete a completed output merely
because a recoverable decode error was reported. Generated reported-error
HEVC inputs produced visible warning data through both Rust and C backend
paths; a generated audio-damage input produced an audio warning. A clean
input produced no warning; a 100,000-byte diagnostic test verified bounded
retention. The GUI warning itself remains unverified while the Mac is locked.
Silent corruption without FFmpeg diagnostics remains undetectable this way.

Additional generated 10-bit HEVC flash/audio-marker inputs are recorded in
`hevc_ingest_sync_2026-11-01.json`. The `round=down` candidate retained a
0.25-second delayed-video marker but produced 71 frames versus 72 for the
current policy in one otherwise clean case. In a generated variable-rate
case, the marker was one 24-fps frame earlier under `round=down` than under
the current policy. Resetting video PTS erased the delayed marker. Audio
markers here were read from FFmpeg-decoded PCM before MP3 sidecar encoding;
these are not complete end-to-end synchronization or hardware measurements.
Neither candidate is enabled. Next: real source comparisons and a precise
A/V timeline policy before changing the default. Reproduce with
`tools/codec_study/ingest_sync_probe.py`; the companion `ingest_probe.py`
can generate non-overwriting video/audio damaged test fixtures for the Rust
integration tests.

## Follow-up: end-of-stream policy

**Host, 2026-11-01:** adding `eof_action=pass` to the `round=down`
candidate avoids a missing final frame in some marker inputs, but produced
one extra frame compared with the FFmpeg 9 reference in all 11 generated
frame-rate cases. The variable-rate flash marker still moved one frame
earlier compared with the current policy. The candidate was not enabled.
See `docs/hevc_ingest_eof_2026-11-01.json` and reproduce with
`tools/codec_study/ingest_eof_probe.py`. These synthetic host results do
not establish the behavior of the original source or Pocket playback.

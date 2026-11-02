# 010: Native C workflow appeared stuck at zero progress

- **Reproduction:** In Tau Encoder 0.2.0, encode a long source with the experimental Python-free Rust + C workflow.
- **Observed:** The video phase displayed zero frames until all video processing finished. On 2026-11-02, host process inspection showed that the reported job had advanced to the FFmpeg MP3 phase; this does not establish Pocket playback or validate the output.
- **Expected:** Show completed video work while encoding and provide an updated estimate when a duration estimate is available.
- **Impact and workaround:** The application looked hung even while it was working. The Python workflow reported segment progress and could be used in the meantime.
- **Change:** Version 0.2.1 reports frames after each successfully accepted segment and estimates the remaining video time from elapsed time. The first update can still take a while, and the duration is approximate.
- **Validation gate:** Host generated-video test confirms multiple segment progress updates; full-length source and Pocket playback remain unverified.

## Source diagnostics follow-up — 2026-11-02

**Host:** the source supplied by the user produced malformed Matroska element
errors under bundled FFmpeg 6.1.1 and FFmpeg 9.0.2, plus HEVC missing-reference
messages during the user's encode. A temporary FFmpeg stream-copy remux
completed and its first 90 seconds decoded without container warnings, but a
full decode of the remux still produced 3,436 missing HEVC reference messages.
The remux does not prove recovery of skipped data; neither output quality nor
Pocket behavior was verified. Tau Encoder 0.2.2 groups common source warning
types and preserves the bounded raw diagnostic text, without claiming repair.
The recommended next test is a clean source. No user media is included here.

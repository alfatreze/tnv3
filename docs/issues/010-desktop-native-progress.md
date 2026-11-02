# 010: Native C workflow appeared stuck at zero progress

- **Reproduction:** In Tau Encoder 0.2.0, encode a long source with the experimental Python-free Rust + C workflow.
- **Observed:** The video phase displayed zero frames until all video processing finished. On 2026-11-02, host process inspection showed that the reported job had advanced to the FFmpeg MP3 phase; this does not establish Pocket playback or validate the output.
- **Expected:** Show completed video work while encoding and provide an updated estimate when a duration estimate is available.
- **Impact and workaround:** The application looked hung even while it was working. The Python workflow reported segment progress and could be used in the meantime.
- **Change:** Version 0.2.1 reports frames after each successfully accepted segment and estimates the remaining video time from elapsed time. The first update can still take a while, and the duration is approximate.
- **Validation gate:** Host generated-video test confirms multiple segment progress updates; full-length source and Pocket playback remain unverified.

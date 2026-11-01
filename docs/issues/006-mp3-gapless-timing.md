# 006: MP3 encoder delay and Pocket audio timing

**Status:** open; host model and source review, not confirmed on Pocket.

The desktop encoder currently writes 48 kHz MP3 sidecars using FFmpeg and
libmp3lame. In three generated HEVC/PCM marker cases, normal FFmpeg decoding
placed an audio pulse at sample 48001, whereas decoding the same MP3 with
`-flags2 +skip_manual` placed it at sample 49106. The difference is 1105
samples, approximately 23.02 ms at 48 kHz. This models the effect of not
applying FFmpeg gapless skip metadata; it does not measure Helix decoding.
The cases include zero and positive/negative video offsets. Reproduce using
`tools/codec_study/mp3_delay_probe.py`; aggregate results are in
`docs/mp3_delay_probe_2026-11-01.json`.

**Code review:** the current Pocket MP3 path skips the Xing/Info frame,
decodes audio through Helix, and advances its audio position using queued
samples without an explicit encoder-delay/padding correction. The video
player uses the audio position as a presentation clock. This makes a timing
difference plausible, but Helix's actual output and hardware scheduling
have not been measured, so neither its magnitude nor its contribution to
the reported freeze is established.

A separate generated zero-offset sample encoded with `-write_xing 0` put the
pulse at sample 49106 under both normal and `skip_manual` FFmpeg decoding.
Omitting the header therefore does not remove the encoded delay in that
test and is not a demonstrated fix. No automatic compensation is enabled.

**Next gate:** run the generated MP3 marker through the actual Helix decoder
and measure emitted PCM and its audio clock, then verify synchronization on
Pocket. Coordinate any decoder or encoder correction with the active player
work; test offsets, padding and end-of-stream before choosing a policy. Until
then, use these findings as a diagnostic lead, not a hardware timing claim.

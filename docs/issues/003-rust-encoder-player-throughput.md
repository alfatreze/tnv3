# 003: Choppy playback and continuing pictures while paused

**Status:** open. **Pocket:** owner reports choppy video from a Rust-app encode
and picture motion that appeared smoother while the player displayed PAUSED
(2026-11-01). No frame-by-frame timing trace or decoder load measurement is
available yet.

**Host file inspection:** the selected local card file is 400×226, 24/1 fps,
34,488 frames, with 32,340 intra pictures (93.8%), no B pictures, and a 48 kHz
stereo 128 kbit/s MP3 sidecar. The C TNV3 checker reports valid. All adjacent
index timestamps differ by exactly 2,000 samples (24 fps at 48 kHz). Mean
video chunk size 6,680 B, 95th percentile 8,284 B; approximate mean video
chunk rate 156.6 KiB/s, excluding audio and storage overhead. The source
reports 24000/1001 fps; the desktop app converts to 24 fps. These data do not
prove a decoder or clock meets its deadlines, nor determine the output's
visual quality. The media remains local and is not a published test vector.

**Code-review hypothesis:** `codec-alternatives` firmware `tvc_player_step`
stops pumping audio on pause but continues decoder steps and calls
`tnv3_present` against the audio position. An existing video backlog can
therefore continue to be decoded and displayed until it catches up to the
frozen audio position. Reduced competing audio work may also change throughput;
that is unmeasured. Continuing pictures while PAUSED deserves a player-side
investigation even if the backlog explanation is confirmed. The unusually high
intra proportion from the Rust baseline may increase hardware work; the
other card samples were made by a different encoder and cannot establish
causation. No per-picture hardware decode times or dropped-frame counts were
collected.

**Safe workaround:** try an existing reference sample on the same core and
compare whether playback and pause behave similarly; do not overwrite the
original test files. **Next gate:** capture shown/decoded counts, audio
underruns, and decode time or missed-presentation deadlines for the same
scene; compare the Rust encode with a reference-encoder output at the same
resolution, frame rate and source segment. Investigate freezing presentation
on pause in the player while preserving safe decoder/slot state. Confirm any
fix on Pocket hardware.

## Follow-up: C-encoder comparison and persistent freeze

**Pocket, owner report:** a C-encoded version of the same film played more
smoothly than the Rust output, but the picture eventually stayed frozen
while audio continued. Exact position and whether another control restored
video were not established. Do not attribute the freeze to B pictures,
compression choices, timestamps, storage or RTL without further evidence.

**Host:** the C version has the same 400×226, 24 fps and 34,488 frame count;
744 intra and 22,780 B pictures. It passed the independent TNV3 checker, and
its video and copied original MP3 were SHA-256 verified on the card. All
34,488 presentation timestamps are present exactly once on the expected
24 fps grid; decode-order reordering is at most two frames. The C software
TVC decoder decoded the complete video to `/dev/null` successfully.

A local copy of the codec-alternatives Pocket glue host simulation had its
test-only four-million-loop automatic quit raised to 150 million and ran the
entire C video twice: once silent and once with a synthetic 48 kHz MP3 of
the same duration. Both reached the end with 34,488 decoded and shown, no
fake card-read discards; the audio run consumed 68,976,000 sample pairs with
no simulated MP3 errors. The simulated decoder is instantaneous, Helix is a
stub, and the MP3 is synthetic: these are **host** checks, not reproduction
of real hardware load or the user's actual audio. No changes were made in
the other agent's player worktree.

**Next investigation:** determine the first failing playback time and whether
seeking or restarting gets past it; collect player state, shown/decoded
counts, decoder status, card-cache state and audio clock at the freeze.
Compare a local excerpt around that time in RTL replay and, if possible, use
an instrumented Pocket build to distinguish a stuck decoder, stalled I/O,
slot/presentation deadlock, and a clock mismatch. Keep existing card media.

## Correction: C/Rust comparison did not isolate the encoder

The Rust desktop output used bundled FFmpeg 6.1.1, while the manually made
C comparison used the system FFmpeg 9.0.2 for source ingestion. Subsequent
synthetic **host** tests found that these two versions produced different
frame sequences through the app's 24 fps Y4M pipeline; see issue 005. Thus
the observed smoother C playback may reflect ingestion as well as TVC encoder
choices or the player. The actual source was unavailable for this follow-up,
so its frame sequence has not been compared and the cause is not established.

## Rust unaligned prediction follow-up

**Host, 2026-11-01:** the Rust encoder previously disabled residual P
pictures for the 400x226 display size of the reported sample. It now
evaluates zero-motion residual prediction using the actual coded padded
reference; generated odd and 400x226 tests agree pixel-for-pixel with the
independent C decoder. A short synthetic 400x226 sample became 23.3% smaller
at nominal QP 30 while encoding more slowly, but C remains substantially
smaller on that case. See `docs/RUST_C_ENCODER_PARITY.md`. This does not
prove the prior playback cause, resolve the C freeze, or establish Pocket
throughput. Next: compare the same actual source under identical ingest and
collect hardware decoder/presentation counters.

## Original-source first-minute ingest check

**Host, 2026-11-02:** the original source is available again. On its first
60 seconds, FFmpeg 6.1.1 and 9.0.2 produced identical Y4M pixel sequences
for the same ingest filter. This limits the version-difference hypothesis
for that window only; it does not measure per-frame PTS, the full file,
hardware decode load or the freeze. See issue 005.

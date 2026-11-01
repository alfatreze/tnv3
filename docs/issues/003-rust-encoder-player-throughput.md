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

# 004: Desktop local preview needs interactive validation

**Status:** open, host backend tested 2026-11-01.

Tau Encoder now includes a local TNV3 viewer with a matching MP3 sidecar and
play/pause controls. The crate's picture decoder is used in display order;
RGB output is BT.601 limited range and the browser audio clock controls frame
presentation. Late pictures are dropped; seeking, replay without reopening,
embedded TAC and Pocket timing emulation are not implemented. The preview is
not a conformance checker.

**Host evidence:** a C-generated synthetic video was decoded through the
player backend and matched the existing Rust Y4M decoder frame-for-frame.
The C and Rust pair generation tests passed with tools taken from the packaged
macOS application, and the C TNV3 checker accepted the outputs. Type checking,
clippy and the macOS bundle build passed. The packaged UI, audio playback,
frame pacing and pause/resume could not be interactively inspected because
the Mac was locked. These claims are not Windows/Linux or Pocket validation.

Next: unlock the Mac, test both encode modes and suffix switching through the
packaged UI, open each pair, listen for MP3 playback and check play/pause,
late-frame behavior, failure handling and appearance. Then test Windows and
Linux and package their helper binaries. Python 3 must be installed separately
for the C mode; the macOS bundle includes the C executables, FFmpeg and
FFprobe.

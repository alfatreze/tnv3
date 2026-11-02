# 002: Desktop pair workflow platform and hardware gates

**Status:** open; host-tested 2026-11-01.

The Tauri desktop app creates matching video-only TNV3 and MP3 sidecars,
using FFmpeg for input decoding and MP3 encoding. Its macOS Apple Silicon
preview includes FFmpeg 6.1.1, dynamically linked LAME 4.0, source archives,
licenses and a build script. A generated 24-frame MP4 with audio produced a
pair accepted by the C TNV3 checker and FFprobe (MP3, 48 kHz, stereo). It
refused existing output files. This is a **host** result, not a Pocket test.

Windows and Linux builds of the preceding app passed CI, but redistributable
FFmpeg helpers for those platforms have not been built or packaged. The
current source can use an installed FFmpeg or `TAU_FFMPEG` there. Audio/video
synchronisation and direct SD-card playback of the resulting pair have not
been confirmed on hardware. Video is converted to 24 fps and fitted within
400×360; source files without audio are currently rejected. The encoder may
still fail on difficult inputs, and progress/cancel are unavailable.

Next: package and independently test redistributable FFmpeg builds on Windows
and Linux, run a longer synchronisation test including nonzero audio offsets,
and validate the pair on a Pocket. Do not call this a cross-platform release
until these gates are met.

## macOS helper update — 2026-11-02

**Host:** Tau Encoder 0.3.0 bundles FFmpeg and FFprobe 9.0.2 with LAME 4.0,
including the source archives and build script. A short generated-media encode
passed with the Python and Python-free C workflows. This does not validate a
full film, source corruption recovery, or Pocket playback. Windows and Linux
native distribution and runtime testing remain open.

## Windows version-check failure — 2026-11-02

**CI:** desktop run 41464080654 passed Linux and macOS, but Windows stopped
before compilation because the version checker assumed LF line endings in
Cargo.lock. The checker now accepts CRLF as well. Windows runtime, packaging,
and external helper validation remain open; a new CI run is the next gate.

# Tau ν: project start (2026-10-11)

Status: fork created, nothing pruned or built yet. Numbers marked [EST] are estimates, not measurements.

## Why a fork
A video decoder is the big new tenant on a chip already at about 71% ALM and 240/308 M10K in the music core. A separate project lets the meter and music hardware be removed instead of gated, and keeps the music core's release cadence untouched.

## Inherited, keep as is
Video timing and double buffering (H2), vblank/beam counters, Talos (blit, scaled blit, CLUT blit, rounded rect, blend), SDRAM/PSRAM controllers and arbiter, CDC primitives, bridge handling, `pcm_fifo`/`sound_i2s`, gain stage, MP3 window unit, VexRiscv SoC and MMIO, cold-code framework, settings/persist, theme system, Check and report codes, ISSP debug probes, packagers, installer, compat manifest and pairing gate.

## Inherited, simplify
- Helios becomes the on-screen display (progress, time, pause, volume, shown briefly on a key press).
- Library: keep the index format (header, CRC, caps, TIM1 thumbnails), `library_core.h` and its tests; model is one flat list of files with title, duration, thumbnail, resume position.

## Inherited, remove in the first pruning pass
Meters (Winamp, Chladni, Layered Wave, VU) and their hardware (spectrum bank, wave block), tempo, FLAC and LPC, ReplayGain, legacy playlist, album/artist/favourite browsing, Cymo resampler and Halcyon EQ (optional later).

## Codec plan (decided: MPEG-4 direct, 2026-10-11)
MPEG-4 Part 2 Simple Profile, I and P frames only. CPU parses the bitstream (host reference decoder done); hardware does IDCT, motion compensation and colour conversion (IDCT and motion compensation exist in simulation). H.264/H.265: not planned. MJPEG is not a product. Status and order: `docs/CURRENT_STATUS.md`.

## Container and tooling
Own simple container (frame index, codec tag, audio track, thumbnail). Written first as a Python reference tool here; Tau Omega gains transcoding (ffmpeg) and library management later, following `docs/CROSS_PROJECT_INTERFACE.md`.

## First steps (done) and what follows
Prune pass on the bitstream, SD throughput probe, reference decoder, hardware golden models: done (V-001..V-021). What follows is in `docs/CURRENT_STATUS.md`.

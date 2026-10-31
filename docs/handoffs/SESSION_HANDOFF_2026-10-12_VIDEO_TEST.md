# Handoff: Tau nu, end of the second session (2026-10-12): video on hardware, parser speed is the open problem

Read this first, then `docs/CURRENT_STATUS.md`, `CLAUDE.md`. Older handoffs (`..._TAU_NU_START.md`, `..._PIXEL_PATH.md`) are still right for rules, owner decisions and gotchas; where they say "uncommitted" or "fit pending", this file wins (everything is committed up to `3ecd051`, nothing pushed; the fit closed). Audit entries `V-023..V-046`.

## Where we are
Real MPEG-4 video decodes and shows on a Pocket: Diagnostics > Tests > VIDEO TEST opens `/Assets/tau/common/tears_90s_mp4v.tnv` (400x168, 24 fps) through the cover data slot, parses 96 frames with the firmware parser, reconstructs them on the hardware decoder and shows them 96 rows down in the displayed framebuffer. It works, it is too slow, and there is no audio or UI yet.

| Build | Parser | VIDEO TEST result (D = P/key frame ms) |
|---|---|---|
| TAU DEV 205 | code and tables in cold PSRAM (uncached) | D139/567 R2 L96 |
| TAU DEV 206 | code and tables in hot RAM (`VT_HOT=1`, stress and other pages dropped), state in PSRAM | D55/218 R2 L96 |
| TAU DEV 207 (built, NOT installed) | as 206 + no per-picture DC/AC clear + 2 KB hot pool for per-macroblock arrays + `W` field | not run |

Budget: 41.7 ms/frame. Simulation (everything hot, stubbed decoder): P about 13 ms, key 45-85 ms. Hardware decoder alone: a prediction 4.1-5.1 k clk_sdram clocks (about 13.6 ms for a frame of 275 predictions if the CPU waits on all), a coded block 610-1,121; the RGB565 output about 2.7 ms; file read 2 ms/frame (not a problem).

## Do next
1. **Owner approval for the card write, then install 207**: `python3 tools/install_dev_core.py work/diagnostics/tau-dev-207/pocket --card /Volumes/TauV --remove "alfatreze.TAU DEV 206" --yes` (dry run already clean). Run VIDEO TEST; result `D<p>/<i> W<w> R<r> L<n>`. Read it: if W is most of D the decoder (hardware) is the limit, so overlap it with the parse (the hooks already let the CPU run ahead; check where `mpv_wait` is called, consider queuing the next macroblock's coefficients while the previous op runs) or shorten the decoder's work; if W is small the CPU parse is the limit: next candidates are the VLC level-1 tables still in PSRAM (4.3 KB, `MP4V_FVS_ADDR`; shrink to 7 bits or put the TCOEF ones hot), the 64 KB pool (DC int32 6.6 KB + AC 46 KB in PSRAM), escape tables `mp4v_lmax/rmax` (scan 102 entries), the AC-prediction divisions (rv32im `div`), the per-macroblock resync check.
2. **Ask the owner what the picture looked like** (colours, orientation, tearing, smoothness): never reported, only the menu was screenshotted. Screenshots land in `Memories/Screenshots/` on the card (mount `/Volumes/TauV`).
3. Product build: the parser must be hot RAM in the shipping config. release has an 8.7 KB heap gap, the diagnostic build only 1-2 KB; decide what leaves hot RAM (`DIAG_DROP`, FLAC, tempo, meters per the scope list) and where the 64 KB state lives (PSRAM uncached costs; consider smaller state: AC rows instead of the whole picture).
4. Audio side and A/V sync (MP3 from the TNV file, audio sample counter as the clock for `fw/video_play.h`'s `audio_samples`), then OSD/library/thumbnails/resume, release packaging (LGPL text and NOTICE, `tools/make_release.py`, `tools/omega_compat.json`), datasheet history purge before the repo is public.
5. Unexplained and low priority: Info UNDERRUNS 1 ALL 2 seen once on 203 (V-021); not reproduced in 204's runs (V-040).

## Practical notes (new this session)
- Build and package a dev core: `python3 tools/package_dev_build.py --number NNN --variant diagnostic --build-flags RAM_192K=1,CLK66=1,SDRAM_BUSY=1,HALCYON_FW=0,LPC_FW=0 --rbf work/diagnostics/nu-mp4v3/ap_core_s1.rbf --rbf-sha256 b6ba7cf15e1162356bb1c0e5d17428dc7250aacf403f182f53c109107e04d90d`; for the hot-parser variant prefix `EXTRA_CFLAGS="-DVT_HOT=1" DIAG_DROP=stress,gap_timing,blit_test,pixel_grid_test,meter_trace,config_export,sweeps,meter_experimental,all_speeds,out16_toggle,gain_toggle` (the tree must be committed or the version says .dirty). Install only with `tools/install_dev_core.py` (dry run first, ask before `--yes`).
- Hot RAM is the scarce resource: `python3 tools/check_targets_link.py` must stay green (release 8.7 KB, diagnostic 1.1 KB, profile 5.5 KB heap gap with the 192 KB flags); parser and tables are cold unless `VT_HOT`; `MP4V_FN/TNV_FN/MPV_FN/MP4V_RODATA/CS()` macros place code, tables and strings.
- Host tests that matter: `make test-host` (includes `test_video_play`, `test_vid_diag`, `test_dec_diag`, `test_mp4v_fast` when `media/tears_90s_mp4v.tnv` exists), `make test-rtl` (about 35 min; includes the Verilator SDRAM tests), `make rtl-lint`. Do not run `test-host` and `test-rtl` at the same time (both build `fw/*.o`). Last full runs green at `a86e9e1`; `test-host` green again after the CHANGELOG change; `test-rtl` not re-run since the RTL stopped changing (it has not changed since `nu-mp4v3`).
- Timing/fit: a fit takes about 80 minutes; failing path last time was one address computation in `tau_mp4v_dec` (fixed by registering). Get path detail with `quartus_sta` on the VM (script pattern in V-033).
- `CHANGELOG.md` was deleted by the owner on purpose (old, unneeded); tests and the dev packager no longer need it; `make_release.py` would.
- The Tau Nu card install path for media is `Assets/tau/common` (shared by the `tau` and `tau_dev` platforms).

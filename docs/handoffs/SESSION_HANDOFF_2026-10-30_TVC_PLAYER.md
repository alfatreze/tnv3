# Session handoff 2026-10-30: TVC player (decoder speed-up, display, register block, whole-core wiring, modular firmware)

Worktree `tau-nu/.claude/worktrees/codec-alternatives`, branch `worktree-codec-alternatives`. Audit V-246..V-268 (this handoff is V-268). **Nothing pushed.** Commits under the owner's identity (`git -c user.name="Abel Santos" -c user.email=alfatreze@gmail.com`), no assistant references. Previous handoff: `SESSION_HANDOFF_2026-10-29_TVC_INTEGRATED.md`. Design: `docs/TVC_PARSER_DESIGN.md` sections 18-21.

## State
| Area | State |
|---|---|
| Decoder speed | Band deblocking (V-247), parse/reconstruct overlap (V-251), residual pre-transform (V-254), deblocking trailing the reconstruction by a macroblock row (V-258). v19 (320x176) on sched_v = 290 k clocks/picture; Level 3 estimate ~30 ms/picture at 100 MHz (budget 33.3 ms; NOT met on clk_sys 66.67). Third macroblock in flight and intra overlap tried/analysed and dropped (V-256/V-257). Slices ~1,244 clk/MB (v19 representative, ~160 bits/MB) |
| Memory | Tau Alpha `sched_v` v1 imported by hash (`tools/verify_sched_v_import.py`), `tau_tvc_memport` (read/write ports), `tau_tvc_mem` (PHY + sched_v). Tau Alpha will swap in a faster scheduler by file (interface frozen; latency may move) |
| Core | `tau_tvc_regs` (IDX/DATA/CMD/STATUS; CMD 1 pic_start 2 slice_go 3 pic_end 4 show 7 clear; 256-word FIFO), `tau_tvc_scan/linegen/video` (400x360 window at 12 MHz), `tau_tvc_core`; `TAU_TVC` branch in `core_game.vh`; bundle `tools/tau_v_tvc_qsf_append.txt`; `tools/vm_map.py` (map-only on the VM) |
| Whole-core synthesis | `quartus_map` 43 min, 0 errors, 22,140 regs, 71 % block RAM, **67 DSP of 66** -> fixed (`multstyle logic` in `tau_mp4v_yuv2rgb`, scan 7 -> 2 DSP, expected ~62); NOT re-mapped, **NOT FITTED** |
| Probe fits | dbkb closes at 10 ns (+1.0 / +1.17 ns); regs probe (two clocks) launched 07:21 on 10-30, result not read (`~/tau-local/regs-1`); scan unit mapped only |
| Firmware (modular, per Tau Alpha's FIRMWARE_MODULARIZATION_PLAN) | `fw/media_src.h`, `fw/tvc_hw.h` (ops table + MMIO impl), `fw/tnv3_core.h` (container, slots, presentation; fixed late-B display bug), `fw/tvc_player_core.h` (non-blocking player), `fw/audio_ops.h` + `fw/audio_mp3_core.h` (MP3 SIDECAR now, TAC later behind the same table), `fw/stage_cache.h` (SD read-ahead with PSRAM staging), `fw/tvc_media.inc` (Pocket glue, stub-tested only) |
| Tests (all green) | `make test-rtl-tvc-top`, `-regs`, `-regs-stress`, `-core`, `-play` (C player -> RTL replay), `-dbkb`, `-resid`, `-mbrec`, `-scan`, `-top-sv` (iverilog only); host: `sim/test_tnv3_core.py`, `test_tvc_player.py`, `test_audio_mp3.py`, `test_stage_cache.py` (in `make test-host`). `test_stage_cache.py` passed after the last macro edit |
| Not built / not done | whole-core FIT and any Pocket run; Pocket bindings for audio (Helix + PCM FIFO, see `fw/av_audio.inc`) and for the main loop / UI; TAC decoder (audio is the sidecar MP3); 800-wide levels (need 37.125 MHz pixel clock, PLL output, video.json); `show` vs vertical blank; seek; data slot 9 in the packager of the TAU_TVC build |

## Decisions made this session (owner)
- Audio first = MP3 **sidecar** file (`film.tnv3` -> `film.mp3`), no container change / codec id; swap to TAC later through `audio_ops`; use it as the perf comparison point.
- PSRAM staging region: default `TVM_STAGE_BASE = 0xA5000000` (die 2; map in KB-225). Owner: "add data slots at will": added slot 9 "Audio sidecar" (mp3) and `tnv3` extension on slot 2 in `dist/Cores/alfatreze.TAU V/data.json` (uncommitted when this was written: commit it).
- Next-step choice left to the assistant: whole-core fit first (long VM job), audio Pocket binding meanwhile.

## Commands
`make test-rtl-tvc-{top,regs,regs-stress,core,play,scan,dbkb,resid,mbrec}` (+ `-mutation`), `make test-rtl-tvc-top-sv` (iverilog), `make test-host`. Whole-core map: `python3 tools/vm_map.py launch NAME --append tools/tau_v_tvc_qsf_append.txt` then `status NAME` (43 min). Fit: `python3 tools/vm_fit.py launch NAME --append tools/tau_v_tvc_qsf_append.txt --seed 1` (state estimate and finish time; the VM is shared with Tau Alpha: `pgrep -a quartus_` first; it refuses while a compile runs). Quartus: `/home/taualpha/intelFPGA_lite/25.1std/quartus/bin`.

## Gotchas (skill KB-222..KB-225)
- iverilog re-evaluates a function in a continuous assignment only when an ARGUMENT changes: pass all inputs as arguments. Verilator cannot model Tau Alpha's chip model (low byte reads 0): use iverilog, `MEM_COMPACT 2, MEM_LOG2 22`.
- zsh does not split `$VAR`: use `${=VAR}`; BSD sed `-i` needs a suffix: use python for edits; never `find /`.
- The SD card interface reports no file size (derive it from the TNV3 header); one DMA read in flight at a time.
- Tau Alpha repo is read-only for us (never push there); take files from tags by hash. Ask the owner before ANY SD write (dry run first: `tools/install_dev_core.py`).

## VM at wrap-up (2026-10-30 ~09:00)
Tau Alpha's `sdram_probe` fit was still running on the shared VM (check `pgrep -a quartus_`), the disk is 88 % full with 258 stage directories in `~/tau-local` (7.3 GB free): a whole-core fit needs space, so the owner should clean old stage dirs (the VM was cleaned the same way before). No whole-core fit was launched.

## Next steps (suggested order)
1. Commit the data.json / `tvc_media.inc` edits; re-run `python3 sim/test_stage_cache.py`; re-map the whole core (`vm_map.py`) to confirm DSP <= 66; read `~/tau-local/regs-1` timing.
2. Launch a whole-core fit (one seed; check the VM is free; state duration and finish time); read timing at 100 MHz.
3. Audio Pocket binding (Helix + PCM FIFO behind `audio_ops`/`mp3_dec_ops`/`pcm_sink_ops`), main-loop glue calling `tvc_player_step` + `stage_pump`, packager for the TAU_TVC build (slots 2 and 9); then a staged Pocket bring-up with the owner (SD install needs approval).
4. Later: TAC decoder (swap), 800-wide window (PLL), seek, faster `sched_v` from Tau Alpha, clk_sys vs 100 MHz decode decision.

# Handoff: codec phase 1c (2026-10-21, end of session): block-syntax RTL done and decoding real bytes

Latest state of the codec track. Read this first, then `SESSION_HANDOFF_2026-10-21_CODEC_PHASE1B.md` and `..._PHASE1.md` (background, gotchas, the study results), `docs/CODEC_AV1_BLOCK_SYNTAX.md`, `docs/CODEC_AREA_PROBES.md`. Worktree `tau-nu/.claude/worktrees/codec-phase0`, branch `codec-phase0`, nothing pushed, everything committed except build outputs. Audit ids V-109..V-073; next id **V-074**. Root the session in that worktree and start every shell command with `cd <worktree> &&` (a bare `cd` elsewhere breaks later commands).

## Done this session (all libaom-verified; libaom is a test oracle only)
* `tau_av1_mc` / `tau_av1_mc2` (8-tap MC, two references): 581 ALMs, 118 MHz, 13 mutants.
* `sim/av1_blk_model.py`: block-level syntax model, symbol-exact against a patched libaom on the nine lean2 streams (1.38 M blocks, 15.1 M symbols). Patch `sim/av1/patches/blocktrace.patch` (now also logs each tile's bytes `TD` and initial CDFs `CX`; the patch FILE was last regenerated before TD/CX, regenerate it from `work/ref/aom-trace` vs `aom-src` if needed).
* `tau_av1_blk` (block-syntax RTL): replays the trace exactly on all nine streams in full (`make test-rtl-av1-blk`, `-mutation` (15 killed), `-full` ~45 min). Optimised in rounds: probe `av1blk7` = 7,755 ALMs, 3,706 regs, 186 M10K (64-row ring), closes 66 MHz (+0.97 ns); 66 MHz is the target clock (unit needs ~17 M cycles/s).
* `tau_av1_syn` (symdec + 16K x 16 CDF RAM + edge-partition derived symbol; symdec gained `req_custom`/`req_p`) and `sim/tb_tau_av1_int.v` / `make test-rtl-av1-int`: REAL tile bytes decode through blk + syn + coef and match libaom, 10 tiles committed test. Key frame 5.3 M clocks (serial decoder), inter frames 0.13-0.83 M.
* Real-bytes check on 30 frames of each study stream (`tools/codec_study/run_int.sh NAME 30`): PASSED action, anime_dialog, anime_wide, dark, dialog. The three 128x128-superblock streams (bw_closeup, bw_garden, bw_room) failed because the edge partition symbol did not know the block was 128x128; fixed (`sym_big` port, committed) and re-running when the session ended; anime_motion had not reported either.
* Tools: `tools/vm_probe.py launch ... --map` (synthesis only, ~5 min, `map NAME` prints the ALM estimate which is within 1% of the fit) and `tools/gen_av1_blk_rom.py`, `sim/av1/blk_vectors.py` (trace -> word stream + `.td`/`.cx` side files).
* Owner listening test: Opus 64 kbit/s vs MP3 128 kbit/s, no audible difference (files in ~/Downloads/codec_audio_test; the blind A-F pack is in `work/codec_phase0/audio`). Opus decode cost on the Pocket CPU is unmeasured.

## Jobs that were running when the session ended (collect first)
* Re-run of bw_closeup / bw_garden / bw_room (30 frames): logs `build/int_bw_*.log`, plus `build/int_anime_motion.log` (may not exist; rerun with run_int.sh). If a stream still fails, debug like the earlier ones: print the first mismatch, then the model with `build/dbg*.py`-style hooks.
* Area ablations on the VM (map-only): `abl_stack abl_ref abl_mvc abl_vt abl_tr abl_coefctx abl_misc abl_part` = the unit with one group of controller states emptied (variants in `build/abl/`, regenerate with the python snippet in this session's history: split the `case (st)` items, replace a group's bodies by `st <= S_IDLE;`). Read with `python3 tools/vm_probe.py map abl_NAME`; baseline map estimate 7,825 ALMs (av1blk7). The drop per group says where the area is.

## Next, in order
1. Collect the jobs above; finish the real-bytes check on all nine streams (consider more than 30 frames overnight: ~5 min per 10 frames per stream, run 3 in parallel).
2. Shrink `tau_av1_blk` (target < 3 k ALMs, ring < 70 M10K): use the ablation numbers; narrow the mode-info record (12-bit vectors, flag out-of-range), 64x64-superblock profile (RL=5 halves the ring; the bw_* streams need RL=6), split the controller. Safety net: trace replay + mutants + real bytes.
3. Frame level: OBU/sequence/frame headers, default CDFs by base_q, CDF averaging/saving, reference management (firmware on the CPU is reasonable). The tile CDFs currently come from libaom's trace.
4. Reconstruction: reference fetch for the MC lanes, intra predictor, deblocking; then integration (decode-ahead queue for the 5.3 M-clock key frame, clocking, memory map, display) and a fit.
5. Owner decisions pending: lean set to freeze (model covers lean2 only), 64x64-only superblocks, audio codec (Opus 64k acceptable by ear), standard AV1 syntax vs a transcoded simpler one.

## Gotchas learnt this session
* Icarus: `always @*` over a huge array makes elaboration hang (use registered/continuous reads); race between a blocking testbench update and a DUT sampling the same edge (use nonblocking); RAMs power up X (clear `coef.lev` etc. by hierarchical reference); a `//` comment inserted mid-line swallows the rest of the line; `small` is a reserved word.
* A stale `.vvp`/`.hex` from `make` happens when prerequisites do not change; delete build outputs when a rule's recipe changes.
* libaom's inverse CDFs vs the specification's increasing form: converted in `blk_vectors.py` (counters and padding unchanged).
* Probes take 40-90 min when timing fails; `--map` answers area in 5 min.

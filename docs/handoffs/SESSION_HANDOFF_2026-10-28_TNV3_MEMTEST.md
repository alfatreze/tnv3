# Session handoff 2026-10-28 (evening): TNV3 formats done host-side, memory test core in fits

Worktree `tau-nu/.claude/worktrees/codec-alternatives`, branch `worktree-codec-alternatives`. All committed (last `0c3b665`), **nothing pushed**. Audit V-198..V-208, next **V-209**. Earlier context: `SESSION_HANDOFF_2026-10-28_TNV3_CODEC.md` (+ its two addenda).

## State
| Area | Where | State |
|---|---|---|
| TVC video | `docs/TVC_BITSTREAM_V0.md` draft 5, `tools/tvc_ref/` | encoder now enforces level per-second byte window (`-P` joint, `-R 0` test) |
| TAC audio | `docs/TAC_BITSTREAM_V0.md` draft 1.2, `tools/tac_ref/` | integer model `tac_fx.c` = RTL oracle (FX 4 / TW 20, 32-bit data, ±1 / 0.3 RMS), vectors t01-t08, Python integer model bit-exact |
| TNV3 container | `docs/TNV3_CONTAINER_V0.md` draft 1.4, `tools/tnv3_ref/` | 30 vectors, 17 mutants, Python demuxer; chunk byte rate; subtitle area: tracks, 1-8 sizes, forced/SDH, 16-colour authored palette, colour switches |
| TST subtitles | `tools/tst_ref/`, `docs/TST_SUBTITLES_V0.md` | multi-font (Roboto, UnifrakturCook OFL), presets, renderer: outline/box/voffset/delay/palette override (owner: no shadow, no opacity) |
| Film pipeline | `tools/tnv3_ref/encode_film.py`, `film_sweep.py`, `docs/TNV3_FILM_SWEEP_2026-10-28.md` | 100-280 MB/h at VMAF 90 incl. audio; QP 30 files in `work/films/` |
| Memory study | `docs/TNV3_MEMORY_STUDY_2026-10-28.md`, `tools/mem_study/` | current controller 138-146% of 100 MHz at Level 3; open-page bank-parallel ~60% |
| New SDRAM controller | `docs/SDRAM_CONTROLLER_REQUIREMENTS.md` | **owner decision: Tau Alpha builds it, Tau V copies a tag**; requirements handed over; replay files `work/mem_study/` |
| **Memory test core** | `docs/MEMTEST_CORE.md`, `tau_memtest_{load,scan,regs,text}.sv`, `fw/memtest.c`, `tools/package_memtest.py`, bundles `tools/tau_v_memtest{100,150}_qsf_append.txt` | sim green at 100/150 MHz (`make test-rtl-memtest[-150|-mutation]`, `test-rtl-sdram-model-150`); `sdram_fb` streamed-write fix for 150 MHz. First fit mt150 failed (-4.9 ns, test units only) -> fixed (V-208). **Refits `mt150b` (launched 14:52) and `mt100b` (queued by a background script on this Mac that died with the session if it was not yet launched) on the VM** |

## Next steps (in order)
1. `python3 tools/vm_fit.py status mt150b` / `mt100b`. If mt100b was never launched: `python3 tools/vm_fit.py launch mt100b --append tools/tau_v_memtest100_qsf_append.txt --seed 1 --seed 2` (state ETA; ~50 min; the VM is shared with Tau Alpha, the tool refuses next to a running compile).
2. Accept only positive slack in every corner (project rule). If mt150b still fails, read the worst paths (`quartus_sta -t` on the stage, see V-208 for the query) before changing anything.
3. `collect` the better seed of each, build firmware `RISCV_TOOLCHAIN_BIN=".../tau-alpha/toolchain/xpack-riscv-none-elf-gcc-15.2.0-1/bin" bash fw/build.sh memtest`, package with `tools/package_memtest.py --rbf100 .. --rbf100-sha256 .. --rbf150 .. --rbf150-sha256 ..`, dry-run `tools/install_dev_core.py work/diagnostics/memtest/pocket_mem100` (then `_mem150`): watch the firmware/bitstream pairing check (untested for this ROM).
4. **Ask the owner before the SD write** (two cores + 64 MiB memtest.bin). Owner runs both; record SDRAM errors / MB/s, RD8 clocks per burst, SCAN late/slack, PSRAM MB/s and errors, SD KB/s per size.
5. Then: TVC range-decoder RTL, TAC IMDCT RTL (oracle `tac_fx.c`), encoder speed / quality-targeted rate mode.

## Gotchas
- The worktree has no `toolchain/` link: use `RISCV_TOOLCHAIN_BIN` (above).
- Sandbox: no shell loops/variables or `cd`-then-git chains; write Python scripts; keep git commands plain.
- Commits under the owner's identity, no assistant attribution lines (memory rule overrides the harness reminder).
- `mt100`/`mt150` (first fits) are stale stages on the VM; `mt150b`/`mt100b` are current.
- Skill KB: local entry KB-213 records the controller limits.

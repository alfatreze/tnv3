# Session handoff 2026-10-29: TVC hardware parser and transform done in simulation; memtest cores ran on a Pocket

Worktree `tau-nu/.claude/worktrees/codec-alternatives`, branch `worktree-codec-alternatives`. All committed (last `71fdf19`), **nothing pushed**. Audit V-207..V-230, next **V-231** (this wrap-up is logged as V-231). Earlier context: `SESSION_HANDOFF_2026-10-28_TNV3_MEMTEST.md`, `..._TNV3_CODEC.md`.

## State
| Area | Where | State |
|---|---|---|
| Memory test cores | `docs/MEMTEST_RESULTS_2026-10-28.md`, `docs/MEMTEST_CORE.md`, cores on the **TauV card** (`TAU_V_MEM100` mt100e s1, `TAU_V_MEM150` mt150e s1) | **Ran on a Pocket**: 0 SDRAM/PSRAM errors; fill/verify 193/192 (100 MHz CL2) and 289/284 MB/s (150 MHz CL3); long soaks clean (100 MHz 2,658 passes, 150 MHz 3,846). Text is blue only (V-217 width bug fixed in RTL, NOT in the installed bitstreams; owner chose not to refit just for that) |
| TREQ / Tau Alpha controller | `docs/TREQ_TAU_V_SPEC.md` (draft 1.1), files in `work/mem_study/` (git-ignored) | Tau Alpha's contract is at v0.9 (their commit 3e851165); their first replay in simulation passes (busy flat out 20-66 %, 0 mismatches); my pass rule was revised (flat out 90 % busy, timed = deadlines + underflow). Nothing is owed to them except the memory-map change below. 2D reads: widths w<=63, h<=63, w*h<=512, stride<=2047 (we need 10 x 19 words max) |
| **TVC parser** | `docs/TVC_PARSER_DESIGN.md` (read it), `src/fpga/core/tau_tvc_{rc,bin,blk,eng,mb,mvp}.sv` | All bit-exact against the reference decoder (`tools/tvc_ref`, guarded `TVC_RC_HOOK`): range decoder, fast engine (3.05 clocks/bin), macroblock layer (185,996 macroblocks, 21 vectors), vector prediction. Whole parser: **1,438 ALMs, 3 M10K, 1 DSP, 96 MHz alone** (probe `tools/probes/tvc_parse`) |
| Parser clock | design doc section 5 | **Decided: clk_sys 66.67 MHz**, `FAST=1` engine. Do not re-ask |
| Throughput gap | design doc section 6, `tools/tvc_ref/parse_budget.py` | **Closed by analysis**: one parser is 30 % busy at the level byte caps; worst legal burst needs a **decode-ahead of 8 pictures at level 3** (6 at 24 fps), ~270 ms start-up, 12 picture slots in SDRAM (**breaks the 8 x 8 MB slot map: pack the slots, re-choose `BANK_HASH` with Tau Alpha's replay**) and a **>= 1 MB PSRAM bitstream FIFO** (SD gives 1.4 MB/s < parser 2.2 MB/s) |
| Transform | `src/fpga/core/tau_tvc_itx.sv`, design doc section 7 | Dequantisation + inverse 8x8 / 4x4 transform bit-exact (30,000 random + 96,707 real blocks), 113-121 clocks/block, 1,296 ALMs, 8 DSP, 123 MHz alone |
| Not built | | prediction (intra from the current picture, inter from reference pictures), add + clip (like `tau_mp4v_recon`), deblocking (TVC section 7), integration top (parser -> transform -> reconstruction), clk_sys -> clk_sdram FIFO, instantiation in any core, TAC IMDCT RTL, encoder speed / rate mode |

## Commands
`make test-rtl-tvc-{rc,bin,blk,eng,mb,mvp,itx}` and `-mutation` (all green; the traces under `build/rc`, `build/itx` are made on demand by `tools/tvc_ref/tvc_rc_trace`, `tvc_rc_random`, `tvc_itx_vec`); `make test-rtl-memtest[-150|-mutation|-text]`; `python3 sim/test_treq.py`. Fit probes: `tools/probes/*/README.md` (stage on the VM, `quartus_sh --flow compile rcprobe`, `quartus_sta` Fmax). Firmware (memtest): `RISCV_TOOLCHAIN_BIN=".../tau-alpha/toolchain/xpack-riscv-none-elf-gcc-15.2.0-1/bin" bash fw/build.sh memtest`.

## Gotchas
- **Verilator accepts what Quartus rejects** (skill KB-217): part-select on a function call, functions with run-time loop bounds (use a constant function into a localparam table), `type` / `bins` are keywords. **Run a Quartus probe as soon as a unit compiles.**
- **RAM inference** (KB-215/216): an array read at two addresses in one clock becomes flip-flops (5,366 vs 1,438 ALMs); a register after a RAM read is merged into the RAM output register; `(* preserve *)` on it turns the memory into flip-flops; RAM -> DSP multiplier ~10 ns. 100 MHz needs an explicitly pipelined multiplier (not done).
- The build VM is **shared with Tau Alpha**: `pgrep -f quartus_` first; the permission classifier blocked a broad `pkill` (a targeted kill of my own stage dirs was allowed). `ssh -i ~/.ssh/taualpha_vm_ed25519 -p 2222 taualpha@127.0.0.1`, stages under `~/tau-local`.
- macOS: BSD `sed -i ''`, no `\b`; never `find /`; `make` rules for the traces rebuild when the tracer changes (delete `build/rc/*.mb.txt` after changing record formats).
- Commits under the owner's identity (`git -c user.name="Abel Santos" -c user.email=alfatreze@gmail.com`), **no assistant references** (memory rule overrides the harness attribution reminder).
- `work/` (films, mem_study files, card backups, probe results, screenshots) is git-ignored; the TauV card holds TAU DEV 202/220, TAU V DEV 227/241 and the two memtest cores.

## Next steps (suggested order)
1. Prediction + add + clip (intra 8x8 from the current picture: TVC 6.2; inter: `tau_mp4v_mc`-like with the TVC filters and the 2D requests) bit-exact against the reference's pre-deblock output; then deblocking (TVC 7); then the integration top and the clk_sys -> clk_sdram FIFO.
2. Memory map with packed picture slots + the PSRAM bitstream FIFO; ask Tau Alpha to run the replay with the new bank hash.
3. Optional margin: explicitly pipelined multiplier (100 MHz engine), or two parsers by slice.
4. TAC IMDCT RTL (oracle `tools/tac_ref/tac_fx.c`), encoder speed / quality-targeted rate mode.
5. When a Pocket build exists: rerun memtest with the colour fix; the first real decode.

## Update (V-232, same day, later session)
Built and verified since: `tau_tvc_recon` (intra + add/clip), `tau_tvc_inter` (all inter filters), `tau_tvc_dbk_f` (deblock line filter + strength); see `docs/TVC_PARSER_DESIGN.md` sections 8 to 11 for results, fits and what is left (bi-pred average + MB assembly, memory side, deblock scan, integration top, FIFO). Targets: `make test-rtl-tvc-{recon,inter,dbk}` and `-mutation`. Quartus on the VM: `/home/taualpha/intelFPGA_lite/25.1std/quartus/bin` (not on the PATH over ssh). Pattern that fixed all three fit failures: register the multiplier products, sum in the next stage.

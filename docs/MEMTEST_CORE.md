# Tau V memory test core (V-207, 2026-10-28)

A developer-only Pocket core that measures what the TNV3 memory study (`docs/TNV3_MEMORY_STUDY_2026-10-28.md`) could only model, on real hardware: SDRAM integrity and throughput at **100 MHz (CL 2) and 150 MHz (CL 3)**, a synthetic 800x720 scan-out's deadlines under load, PSRAM throughput and integrity, and SD-card read speed with larger requests. Two cores from one firmware: `alfatreze.TAU_V_MEM100` and `alfatreze.TAU_V_MEM150`, platform `tau_v_memtest`.

## Why a separate core
The player's SDRAM clock also runs `mp3_fb` (draw engine and scan-out fill), whose worst paths already take about 10 ns at 100 MHz: it cannot run at 150 MHz. This core has no `mp3_fb`: the screen is `tau_memtest_text` (25 x 22 characters of the core's 16x16 font from on-chip RAM, the usual 400x360 timing), so the SDRAM clock domain holds only the controller, the arbiter, the CPU bridge's SDRAM side and the test units.

## Hardware (bundles `tools/tau_v_memtest100_qsf_append.txt`, `..._150_...`: macros `TAU_MEMTEST`, `TAU_MEM150`)
| Unit | File | What it does |
|---|---|---|
| load generator | `tau_memtest_load.sv` (arbiter3 decoder master) | FILL / VERIFY sequential bursts, random reads (checked) and random writes; every word = hash(address, seed), so any read is checked without a reference; reports clocks, words, bursts, errors, first error (address, value read, value expected) |
| scan-out | `tau_memtest_scan.sv` (arbiter3 framebuffer master, the priority port) | a 4:2:0 picture shown at a line rate: per line `lw` luma words and on even lines `cw` Cb + `cw` Cr words; counts late lines and the worst slack |
| registers | `tau_memtest_regs.sv` | the decoder register bus of `mp3_soc` (IDX/DATA/CMD/STATUS at 0x1F0..0x1FC): parameters, results, start/done across clock domains, text cells |
| screen | `tau_memtest_text.sv` | text from on-chip RAM, no SDRAM traffic |
| controller | `sdram_fb.sv` (shared) | unchanged at 100 MHz; at 150 MHz CL 3 and **a fix**: the streamed-write source pipeline now waits the extra tRCD clocks (`WSRC_HOLD`), otherwise written data is shifted by one word (proved: the 150 MHz controller test fails without it) |
| clock | `mf_pllbase_0002.v` | `TAU_MEM150`: outclk 3 = 150 MHz (600 MHz VCO / 4) |

Verification (Verilator, real `sdram_fb` + `sim/sdram_chip_model.v`, now parametric in CL / tRCD / tRP): `make test-rtl-memtest` (100 MHz), `make test-rtl-memtest-150`, `make test-rtl-memtest-mutation` (3 load-generator mutants killed), `make test-rtl-sdram-model-150` (the controller's own contract at 150 MHz). Simulation numbers: FILL 1.03 clocks per word; random 8-word reads 41 clocks per burst at 100 MHz (32 at 150 MHz) with the scan stream competing; scan-out 0 late lines with 1,051 clocks of worst slack at 100 MHz.

## Firmware (`fw/memtest.c`, `bash fw/build.sh memtest`)
On boot: SDRAM fill + verify of 62 MB (five power-of-two regions; the scan planes use the last 2 MB), random 8- and 32-word reads and 8-word writes (MB/s, clocks per burst), random 8-word reads with the 800x720 / 60 Hz scan stream running (late lines, worst slack), PSRAM write/read MB/s on 1 MiB and a fill/verify of all 32 MB (guard words skipped), SD reads of `memtest.bin` (data slot 2) in 4, 16 and 64 KiB requests (KB/s, slowest request). Then it soaks the SDRAM (fill + verify, new seed every pass) and shows the pass and error counts; **A** runs the suite again.

## Reading the results
- Any SDRAM or PSRAM error is shown in red with the first failing address and values. At 150 MHz an error means the board's signal margins do not hold at that clock (the controller is proven in simulation).
- `SCAN ... LATE 0` means a 800x720 4:2:0 display at 60 Hz kept every deadline while the random-read load ran: compare its slack with the study's numbers.
- The random-read MB/s and clocks per burst measure the existing controller's per-request cost on the real chip (simulation: n + 19 clocks per read).
- SD KB/s per request size answers whether larger reads help (Player 2 measured 2,433 KB/s with 4 KiB reads).

## Not covered
Timing closure at 150 MHz is decided by the fit; the SDRAM pins have no I/O timing constraints (as in every Tau build), so a clean fit does not guarantee the board interface: the soak does. No audio, no player.

# TNV3 memory study: SDRAM load of the decoder and scan-out (2026-10-28)

Question (scope doc E-G, adversarial review C5): can the Pocket's SDRAM carry a TNV3 decoder plus scan-out at levels 2 and 3, with the existing controller or a new one, at 100 MHz or faster? Method: real traffic (motion vectors of real TVC streams) replayed through a command-level SDRAM model, calibrated against the real controller RTL in Verilator. No Pocket run yet: the Pocket test core (step B) only has to confirm these numbers and test clocks above 100 MHz.

## 1. Inputs and tools (`tools/mem_study/`)
- **Traffic.** `tvc_trace.c` = the TVC reference decoder with a hook in `tvc_recon_mb` that writes, per macroblock, its type, lists and vectors. The hardware decoder is assumed to (a) fetch motion-compensation references from SDRAM per macroblock and list (luma 16x16 + 3 with quarter-sample 4-tap filters, chroma 8x8 + 1, only where the vector is fractional, TVC 6.3), (b) write each reconstructed macroblock once after on-chip deblocking (a one-macroblock-row pipeline with line buffers), (c) keep intra prediction, deblocking and the bitstream on chip, and (d) scan out the displayed picture's 4:2:0 planes at the display refresh rate. CPU, audio and subtitle traffic are not counted (all small).
- **Model.** `sdram_model.c`: the Pocket's 32M x 16 SDRAM (4 banks x 8192 rows x 1024 columns), timings from the chip data in `sdram_fb.sv` (tRCD 18, tRP 18, tRAS 48, tRC 60, tRFC 80, tWR 15 ns; tRRD 12 ns assumed), CL 2 up to 133 MHz and 3 above, refresh 8192 per 64 ms. Policies: `current` (the existing controller: every request serial, overheads from the calibration), `close` (auto-precharge after each burst, banks overlapped), `open` (open page per bank; activate and precharge of other banks overlap the data of earlier requests, in order, with a look-ahead of N requests; refresh only when due). Layouts: linear planes or 2 KB tiles (one SDRAM row each, banks swizzled). Bursts are per footprint line, split at row/tile edges, merged when contiguous.
- **Calibration.** `tb_sdram_calib.v`: the real `sdram_fb` against `sim/sdram_chip_model.v`, back-to-back requests at random rows: **read = n + 19 cycles, write = n + 11 cycles** for n words (n = 1..400), i.e. the refresh before every read, activate, CAS, precharge and handshake, never overlapped. These are the `current` policy's overheads.
- **Matrix.** `run_study.py TRACE...` prints the table below. Workloads: *film* = the trace's own rate (24 fps) + its picture scanned at 48 Hz; *Level 2 max* = 43,500 macroblocks/s + 800x450 at 60 Hz; *Level 3 max* = 67,500 macroblocks/s + 800x720 at 60 Hz. Decode cost per macroblock comes from the trace, so it carries that content's mix of skip / inter / bi-predicted macroblocks and vector fractions.

## 2. Results in detail, Tears of Steel (whole film, 17,529 pictures, 800x334, TVC QP 30, VMAF 93.8)
18.4 M macroblocks: 92.0% inter or skip, 30.1% bi-predicted; 74 SDRAM requests and 474 words (948 bytes) per macroblock on average.

| Controller / layout | SDRAM cycles per MB | film (own rate + 48 Hz) | Level 2 max | Level 3 max (of which scan-out) |
|---|---|---|---|---|
| **current controller (sdram_fb), 100 MHz** | 1624 | 51% | **88%** | **138%** (28%) |
| close-page, linear | 758 | 29% | 50% | 78% (26%) |
| open-page, linear, look-ahead 1 | 505 | 23% | 38% | 61% (26%) |
| **open-page, linear, look-ahead 4** | 499 | 22% | **38%** | **60%** (26%) |
| open-page, look-ahead 4, +2 cycles per request (pipeline bubbles) | 649 | 26% | 45% | 70% (27%) |
| open-page, look-ahead 16 | 499 | 22% | 38% | 60% (26%) |
| open-page, tiled 64x32 / 32x64 / 16x128 | 502-503 | 23% | 39% | 61% (27%) |
| open-page, linear, 133 MHz | 506 | 17% | 29% | 45% (20%) |
| open-page, linear, 150 MHz (CL 3) | 508 | 15% | 26% | 40% (18%) |
| current controller at 150 MHz (overheads x1.5) | 2194 | 44% | 76% | 118% (19%) |

(Percent = share of the SDRAM clock; 100% = the bus is never idle.)

### 2b. All three films (whole files at TVC QP 30, from the film sweep `docs/TNV3_FILM_SWEEP_2026-10-28.md`)
| Film | MBs (inter / bi) | cycles per MB, current / open-page | real film rate: current / open-page | Level 2 max: current / open | Level 3 max: current / open (+2 cycles per request) / open at 150 MHz |
|---|---|---|---|---|---|
| Tears of Steel, 800x334, 24 fps | 18.4 M (92.0% / 30.1%) | 1624 / 499 | 51% / 22% | 88% / 38% | 138% / 60% (70%) / 40% |
| B&W TV episode, 640x480, 23.976 fps | 44.4 M (92.8% / 38.6%) | 1681 / 503 | 60% / 26% | 91% / 38% | 141% / 60% (71%) / 40% |
| Anime episode, 800x450, 23.976 fps | 50.1 M (94.7% / 53.6%) | 1750 / 509 | 75% / 31% | 94% / 39% | 146% / 61% (72%) / 41% |

The three films agree within 2 points for an open-page controller: its cost per macroblock is set by the words moved (474-486 per macroblock), not by the content. The current controller degrades with bi-prediction (more, shorter requests): the anime episode at its own rate already needs 75% of 100 MHz.

## 3. Findings
1. **The existing controller cannot carry Level 3 and has no margin at Level 2.** Its per-request cost (n + 19 for a read: a refresh before every read and nothing overlapped) makes motion compensation's short bursts (6.4 words on average) cost 3.3x their data. A real 24 fps film at 800x334 fits (51%), the Level 2 maximum would leave 12% for the CPU, refresh jitter and everything else, and Level 3 needs 138%. Raising its clock to 150 MHz does not fix it (118%).
2. **An open-page, bank-interleaved controller at 100 MHz is enough for both levels**: 38% at the Level 2 maximum and 60% at the Level 3 maximum, 70% with two cycles of pipeline overhead per request. Its cost per macroblock (499 cycles for 474 words) is within 5% of the data itself: the bus, not activates or precharges, is the limit, so look-ahead beyond one request and tiling bring nothing in this model.
3. **The levers left are clock and words, not scheduling.** 133 MHz takes Level 3 to 45%, 150 MHz (a clean divide of the existing 600 MHz VCO; CL 3) to 40%. Fewer words: reference footprints overfetch (19x19 + word alignment = 1.4-1.6x the 16x16 block, chroma 9x9 for 8x8, bi-prediction doubles it); a small reference cache shared by neighbouring macroblocks (not modelled; estimate 20-35% fewer read words) would bring Level 3 near 50% at 100 MHz.
4. **Scan-out is a quarter of the load at 800x720 / 60 Hz** (52 MB/s, 26%). Showing 24 fps content at 48 Hz cuts it by a fifth; letterboxed content only fetches the picture rows.
5. Close-page (auto-precharge) is a cheap middle ground (78% at Level 3) but not enough with margin.

## 4. Recommendation (for the roadmap)
- **Write a new SDRAM controller for TNV3** (it fits Tau Alpha's planned T5 controller rewrite; build it once, there or here, and share it by copy under the cross-project rule): open page per bank, bank-parallel activate/precharge with at least one request of look-ahead, refresh only when due, read bursts of any length without a refresh in front, write bursts from the requester, a multi-master front end with a guaranteed scan-out share (scan-out is hard real time) and a decoder port that streams footprint lines back to back. Target from this study: under 2 cycles of overhead per request at 100 MHz; verify against a timing-checking SDRAM model (Tau Alpha decision D-T06).
- Keep 100 MHz as the baseline; test 150 MHz on a Pocket (step B) as headroom for Level 3 with bi-prediction-heavy content.
- Plan a reference cache in the motion-compensation unit only if the hardware decoder's measured load needs it.

## 5. Limits of this study
The model is in-order and assumes the command bus never limits (one command per cycle against bursts of about 6 words: true on average, not in every cycle); scan-out and decode are costed separately and added (their interleaving can close rows early: small for long scan bursts, estimate a few percent); CPU traffic, audio and subtitles are not counted; three films (live action / CG, B&W TV, anime), one QP each (30). The `current` row is calibrated cycle-exactly; the other rows are a model of a controller that does not exist yet.

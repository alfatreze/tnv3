# SDRAM controller requirements (from Tau V for the shared new controller), v1, 2026-10-28

**From:** Tau V (tau-nu), TNV3 video decoder track. **To:** Tau Alpha, which builds the controller (its Talos plan's phase T5); Tau V copies a tagged version under the cross-project rule (copy once, reference by path + commit + SHA-256, no shared files).
**Evidence:** `docs/TNV3_MEMORY_STUDY_2026-10-28.md` (tau-nu): the existing controller (`sdram_fb` = Tau Alpha's `tau_sdram_ctrl`) measured in Verilator at **n + 19 cycles per read and n + 11 per write** of n words, which puts a TNV3 Level 3 decoder plus 800x720 scan-out at **138% of 100 MHz**; an open-page, bank-interleaved controller modelled on the same real traffic needs **60%** (Level 2: 88% vs 38%).
Requirement keywords: **must** (needed by Tau V), **should** (strongly wanted), **may** (nice to have). Numbers marked *estimate* are not measured.

## 1. Device and platform
- Pocket SDRAM: 64 MB, 32M x 16, 4 banks x 8192 rows x 1024 columns. Timings as in the current controller (tRCD 18, tRP 18, tRAS 48, tRC 60, tRFC 80, tWR 15 ns; refresh 8192 rows per 64 ms); confirm tRRD, tWTR, tMRD and the CL limits from the datasheet.
- **Must** run at 100 MHz with CL 2. **Must** be parametric in clock (timings given in ns, converted at elaboration) and CAS latency (2 or 3), so 133 and 150 MHz can be tried without code changes (150 MHz = 600 MHz VCO / 4, CL 3). **Should** close timing at 150 MHz on the Cyclone V (5CEBA4F23C8) on its own, so it has margin at 100 MHz inside a full core.
- **Must** register the client side of every port (requests in, read data / valid / ready out): with today's combinational request -> arbiter -> controller path the Tau V 150 MHz test core failed timing by 2-4.9 ns on exactly those paths (V-207/V-208, skill KB-213); a request derived combinationally from an 'available' output is also a loop.
- **Must** register all SDRAM pins in I/O cells (fast input/output registers) and come with SDC constraints for the SDRAM interface (clock-to-pin delays and the clock's phase, PLL shift or DDIO), which the current core lacks.

## 2. Scheduling (what makes it fast)
- **Must keep rows open** per bank (open-page policy) and precharge only on a row conflict, before a refresh, or when a timing parameter forces it. A close-page mode **may** exist as a parameter (modelled at 78% of 100 MHz for Level 3: not enough as the default).
- **Must overlap bank operations:** while one request transfers data, ACTIVE/PRECHARGE for the next request on another bank proceed (bank parallelism), with a look-ahead of **at least one queued request** (the model shows 1 is as good as 16 for this traffic).
- **Must refresh only when due** (distributed, one per 7.8 us on average), never as a prefix of every read. **Should** allow postponing up to 8 refreshes (JEDEC) so a refresh does not split a burst or delay an urgent scan-out read.
- **Must** chain back-to-back bursts to open rows without idle cycles on the data bus (read after read, write after write), and pay only the device's minimum turnaround for read/write switches.
- **Per-request overhead target:** at most **2 controller cycles per request** beyond the device's own timing on a stream of short requests (6-10 words) to open rows; the model's ideal is 0 (499 vs 649 cycles per macroblock at +2: Level 3 at 60% vs 70% of 100 MHz).

## 3. Requests and data
- Word-addressed (16-bit), 25-bit address; lengths **1..512 words** per request. **Should** accept requests that cross a 1024-word row boundary and split them internally (clients then need no page arithmetic); if not, it **must** reject or flag them (the current controller hangs or wraps).
- Writes: data streamed from the client with a ready/valid (or pull-address) handshake at one word per clock; **byte enables** per word (the CPU needs them; decoder ports always write full words).
- Reads: data returned **in order per port** at one word per clock with valid and last; the read latency from grant to first word **should** be the device minimum plus at most 2 cycles.
- **Must** report completion per request (for fences), and **should** expose per-port counters (requests, words, busy cycles, worst wait) for diagnostics, readable through MMIO.
- No ordering guarantee is needed **between** ports; within a port, requests complete in issue order and a read after a write to the same address returns the new data.

## 4. Ports and arbitration
- **Must** support **at least 6 ports** (parameter), all in the controller's clock domain (clients do their own clock crossing). Tau V's use at Level 3, from the study:

| Port | Client | Traffic (Level 3 maximum, *estimate from the model*) | Requirement |
|---|---|---|---|
| 0 | display scan-out (800x720 4:2:0, 60 Hz) | 52 MB/s, lines of 200-400 words, one line per 22.7 us | **hard real time**: never late |
| 1 | motion-compensation reference reads | about 39 MB/s; 2.9 M requests/s of 4-10 words (median 8) | throughput |
| 2 | reconstruction writes | about 26 MB/s; 2.2 M requests/s of 4-8 words (fewer and longer if the client buffers a macroblock row) | throughput |
| 3 | CPU (byte enables, single words and cache lines) | small | low latency when idle |
| 4 | general DMA (bitstream ring, audio, subtitles, blits) | under 2 MB/s for Tau V | throughput |
| 5 | spare | | |

- **Scan-out guarantee (must):** port 0 has a deadline mode: when its client signals *urgent* (FIFO below a threshold), it is granted at the next request boundary, ahead of all others. The worst-case wait of an urgent port 0 request **must** be bounded and documented (the longest request allowed on other ports + one refresh + device timing); per-port **maximum grant length** parameters (e.g. 64 words for decoder ports) make the bound small. Acceptance: Section 6.
- Between the other ports: **round-robin or weighted** (weights as parameters), granted **per request** so short requests of ports 1 and 2 interleave; a port may hold the grant for several queued requests to the same open row (**should**, it saves turnarounds).
- Each port **should** have a small request queue (2-4 entries) so the scheduler sees the next request early (the look-ahead of Section 2).

## 5. Parameters (elaboration time)
Clock MHz; CAS latency; device timings in ns; number of ports; per port: priority class (urgent-capable or not), weight, maximum grant length, queue depth, write byte-enable support; page policy (open / close); refresh postpone limit. Defaults for Tau V: 100 MHz, CL 2, 6 ports, open page, port 0 urgent-capable.

## 6. Verification and acceptance (what Tau V will check before copying)
1. A **timing-checking SDRAM chip model** (Tau Alpha decision D-T06) that flags every violation of tRCD, tRP, tRAS, tRC, tRRD, tWR, tWTR, tRFC, CL, the refresh rate (8192 per 64 ms) and illegal commands for a bank's state, plus a data model (read returns what was written, with byte enables). The controller passes it with zero violations in all tests below, at 100 and 150 MHz.
2. Directed tests: every request length 1..512, row-crossing requests, read/write turnarounds, byte enables, refresh postponing, back-to-back to the same and different banks, all ports at once, reset.
3. **TNV3 replay (throughput):** the request stream `tau-nu/work/mem_study/tnv3_tears_q30_48pic.req` (48 pictures from the middle of Tears of Steel, 800x334, TVC QP 30; 3.74 M requests; 8 bytes each: `u8 port` (1 reference read, 2 reconstruction write), `u8 dir` (0 read, 1 write), `u16 words`, `u32 word address`; SHA-256 `af00952b59aeb29cdb6e8692c4d64e6c65b102cd78d5d5cfa6c403bb2c9897ec`) issued in order on ports 1 and 2 as fast as the controller accepts. The model (`tools/mem_study/sdram_model`, open page, look-ahead 4, 100 MHz) completes it in **25.5 M cycles** (506 per macroblock); the existing controller needs about 82 M. **Accept: at most 28.1 M cycles (model + 10%) at 100 MHz.**
4. **Scan-out under load:** the same replay with port 0 fetching `tau-nu/work/mem_study/scan_800x720_1frame.req` (one 800x720 4:2:0 frame: 1,846 requests; SHA-256 `b74303f1571e0af5be4f4e2b839029d633af715c49c3465c8e870ae6e39ac964`) repeated at the pace of a real 60 Hz display (one 800-sample luma line and every second line the two 400-sample chroma lines, every 22.7 us (735-line frame at 60 Hz, as Tau Alpha's test/720 timing); 2-line FIFO; urgent below 1 line), plus the CPU port issuing a random single-word read every 2 us. **Accept: zero scan-out underflows over 20 frames, and the throughput of 3 still within model + 25%.**
5. Mutation tests on the scheduler (the project's discipline), and an area/timing probe: ALMs, M10K, Fmax (*estimate* of the budget: under 2,000 ALMs and 4 M10K for 6 ports).
6. On a Pocket: a soak (pattern write/read over all 64 MB, all ports active) at 100 MHz and, if 3 holds, at 150 MHz.

To regenerate the request files (tau-nu, worktree `codec-alternatives`): `tools/mem_study/tvc_trace TEARS_Q30.tvc tears.trace`, then `tools/mem_study/sdram_model tears.trace --skip 9600 --frames 48 --dump X.req` and `sdram_model tears.trace --frames 0 --scan 800x720@60 --dump SCAN.req` (the TVC file is the whole-film QP 30 encode from `tools/tnv3_ref/encode_film.py`). Address layout: planes linear, row-major, each picture buffer at an 8 MB boundary.

## 7. Deliverables
RTL (MIT), the chip model, the tests above with their results, an interface document (signals, timing diagrams of request/grant/data, the worst-case port 0 wait), the area/Fmax probe, and a tagged commit with SHA-256 of the files. Tau V copies that tag and records it in `docs/UPSTREAM_TAU_ALPHA.md`.

## 8. Open points for Tau Alpha to decide
- Full-page bursts with BURST TERMINATE versus fixed BL-8 chained: either is fine if Section 2's targets hold.
- Whether the CPU port keeps today's mailbox semantics or gets a cache-line burst interface.
- Address mapping (row/bank/column bits): Tau V's numbers assume bank = word address bits 11:10 (consecutive 2 KB blocks rotate over the 4 banks); another mapping is fine if the replay target holds, but tell us.
- Clock-domain plan for Talos and the CPU bridge if the controller moves to 133/150 MHz.

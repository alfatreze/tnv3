# TREQ: the timed request stream with markers (Tau V side), draft 1, 2026-10-28

For the real-time replay of the shared SDRAM controller (Tau Alpha contract `SDRAM_CONTROLLER_CONTRACT.md` section 7.4). The contract names the format (16 bytes: port, direction, words, address, release time ns, flags; bit 0 = fence) and asks Tau V for macroblock and picture markers, dependencies, deadlines, decode-ahead depth, worst-case traces and the four-vector decision. This is Tau V's answer. **The record layout below is a proposal for Tau Alpha to adopt or change**; everything else is what Tau V commits to.

Tools (this repo): `tools/mem_study/sdram_model --treq FILE` writes the stream from a TVC trace; `tools/mem_study/gen_worst_trace.py` makes the synthetic worst-case traces; `sim/test_treq.py` checks files (layout, markers, release times, deadlines, fences, equality with the untimed `.req`, 7 mutants); `tools/mem_study/treq_stats.py` reports requests per macroblock and the 2D fusing. Audit entry V-219.

## 1. Record (16 bytes, little endian)
| Offset | Field | Meaning |
|---|---|---|
| 0 | `u8 port` | 0 scan-out, 1 reference read, 2 reconstruction write, 3 CPU, 4 DMA. These files carry ports 1 and 2 only; the harness generates 0, 3 and 4 itself (section 5) |
| 1 | `u8 kind` | 0 read, 1 write, 2 marker |
| 2 | `u16 words` | request: words (1..512, one burst, never across a 1,024-word page); marker: marker type |
| 4 | `u32 address` | request: 16-bit word address; marker: its value |
| 8 | `u32 release_ns` | earliest time the record may be offered, from the start of the stream; marker PIC_END: the deadline (section 3). A stream longer than 4.29 s is cut into segments |
| 12 | `u16 flags` | bit 0 `FENCE_PORT`: all earlier requests on this port are done (the contract's fence; not used by these files); bit 1 `FENCE_RD`: all earlier READ requests of ports 1 and 2 are done |
| 14 | `u16 aux` | request: index of the macroblock in its picture (0..2,249); PIC_START: picture type (0 I, 1 P, 2 B); STREAM: format version (1) |

Markers: **STREAM** (type 0, first record only): address = frame rate x 1000, release = macroblock rate (MB/s), flags = decode-ahead depth A. **PIC_START** (type 1): address = picture number (decode order, from 0), release = the earliest start. **PIC_END** (type 2): address = picture number, release = the deadline.

## 2. Offer rules (what the harness does)
1. Records are offered **strictly in file order**; a record is not offered before its `release_ns` (in the controller clock: `ns x clock`).
2. A `FENCE_RD` record waits until every earlier read of ports 1 and 2 has delivered its last word (`r_last`).
3. **PIC_END is a barrier:** it is reached when every earlier record of the file is done (reads delivered, writes `done`); no later record is offered before it is reached. The time it is reached is compared with its deadline.
4. Ports 1 and 2 are the decoder's two ports; the order across them is the file order.

## 3. Release times, dependencies and deadlines (what Tau V commits to)
* **Macroblock release:** macroblock j of a picture is released at `pic_release + j / MB_rate`. MB_rate is the TVC level's maximum (**67,500 MB/s at level 3**, 43,500 at level 2): the fastest the parser may deliver, hence the hardest case. A slower real parser only helps.
* **Data dependency:** the reconstruction write of a macroblock needs its prediction, so **the first write of each macroblock carries `FENCE_RD`** (all its reads, and every earlier macroblock's, are done). The macroblock's other writes follow in order.
* **Picture dependency:** a picture reads its reference pictures, which must be completely written: PIC_END of the previous picture (decode order) is a barrier (rule 3). This is conservative: a real decoder may overlap picture n+1's early macroblock rows with picture n's last rows, which only makes the real case easier.
* **Deadline:** picture n (decode-order index) has `release = n / fps` and `deadline = (n + A + 1) / fps`, **A = decode-ahead depth in pictures, default 1** (the picture may use up to A further picture periods of buffering: SDRAM holds 8 picture slots, A up to 3 is possible; Tau V has not chosen the final value). The display order differs from the decode order around B pictures by at most the number of consecutive B pictures; the decode index is used and the error is at most that many periods (state it if it matters; the worst-case traces are B-heavy, so use A = 1 there).
* **Pass (revised after Tau Alpha's first replay, V-227; the first draft measured busy time in timed mode and could not pass):** at the level 3 maximum the parser's release span alone is 2,250 / 67,500 s = 33.3 ms, the whole 30 fps picture period, so any busy-time rule that includes the release pacing fails by construction. Two modes instead. **Flat out** (release times ignored; order, fences and barriers kept): for every picture the busy time `reached(n) - reached(n-1)` is at most **90 % of the picture period 1/fps** (the 10 % slack rule), with scan-out, CPU and DMA running; report p50 / p99 / max. **Timed** (release times kept): every PIC_END reached by its deadline, no scan-out underflow; also report the tail `reached(n) - release of its last macroblock` (the latency of the picture's last macroblock after the parser has released it).

## 4. Traces delivered
All with the same format; **numbers are the idealised model** (`sdram_model`, open page, look-ahead 4, 100 MHz CL2, decode traffic only, no scan-out, no command bus: an optimistic floor, the review estimated 88-98 % for the worst B pictures with realistic overhead). Level 3 picture period at 30 fps = 3,333,333 cycles.
| Trace | What | requests / MB | words / MB | cycles / MB | worst picture (cycles, share of the period) |
|---|---|---|---|---|---|
| `tnv3_tears_q30_48pic` | real clip, 48 pictures from the middle of Tears of Steel (800x334, TVC QP 30, 24 fps; the untimed `.req` of the requirements, SHA-256 `af00952b...97ec`, is reproduced exactly) | 74.3 | 480.9 | 506 | 625,942 (1,050 MB, 24 fps: 15 %) |
| `worst/allbi` | level 3 (50x45 MB), I P (B B P)..., every B macroblock bi-predicted, odd integer parts, quarter-sample fractions in x and y, vectors over the whole level range | 77.3 | 528.4 (a B picture: 752) | 546 | 1,463,869 (44 %) |
| `worst/static` | every macroblock SKIP, zero vector | 62.7 | 376.0 | 391 | 898,015 (27 %) |
| `worst/scenechange` | every 8th picture all intra, the rest single-list inter with small vectors | 63.4 | 418.0 | 433 | 1,051,088 (32 %) |
| `worst/pan` | every macroblock inter, one large vector (all windows unaligned) | 65.7 | 443.6 | 459 | 1,059,135 (32 %) |
| `worst/random` | 30 % bi, 60 % single, 10 % intra in B pictures | 61.4 | 403.1 | 417 | 1,024,864 (31 %) |
The files are in `work/mem_study/` (git-ignored, large: 60 MB for the clip, 51-64 MB each); regenerate with the commands in section 8. Tau Alpha can be given copies and SHA-256 as for the `.req` files.

## 5. What the harness generates itself (Tau V's parameters, from the requirements and the contract)
* **Port 0, scan-out:** 800x720 4:2:0 at 60 Hz: one 800-sample luma line, and on every second line the two 400-sample chroma lines, every 22.7 us (735-line frame); 2-line FIFO, urgent below 1 line; the request pattern is `scan_800x720_1frame.req` (1,846 requests, SHA-256 `b74303f1...ac964`).
* **Port 3, CPU:** a random single-word read every 2 us.
* **Port 4, DMA:** 2 MB/s (bitstream and audio into SDRAM). *Direction and burst size are estimates:* sequential writes in 64-word bursts.
* **Refresh** as the controller does it.

## 6. 2D reads (contract v0.6: w <= 63, h <= 63, w x h <= 512 words, stride <= 2,047 words)
The files hold 1D requests (one per window row). A 2D port fuses them by this rule: **within one macroblock, consecutive port-1 reads with the same word count and a constant address step of 1..2,047 words are one 2D request** (w = words, h = rows, stride = the step). Measured (`treq_stats.py`; spurious pairs of two rows make the fusing slightly optimistic): the clip needs **36.2 requests per macroblock instead of 74.3**; reads fall from 42.3 to 4.2 per macroblock (10.1 rows per 2D read), **writes stay 32 per macroblock (16 luma + 8 + 8 chroma rows) and become the majority.** Largest block: **10 words x 19 rows = 190 words, stride 400** (800-sample luma planes; 814 once in `allbi`), so the v0.6 widths are ample and **no request is ever rejected as `bad_req`**. Consequence for Tau Alpha: with 2D reads only, a write buffer (4 macroblocks = about 3 M10K, 32-word writes, about 6 requests per macroblock) or 2D writes is what takes the request count below 10 per macroblock. Tau V's plan is the write buffer; 2D writes are the alternative if the buffer proves too expensive.

## 7. Answers to the contract's list
| Contract asks | Answer |
|---|---|
| Macroblock and picture markers, dependencies | sections 1 to 3 (PIC_START, PIC_END, per-macroblock release and index, FENCE_RD, the picture barrier) |
| Deadlines, decode-ahead depth | section 3: `(n + A + 1) / fps`, A = 1 by default (parameter in the STREAM record) |
| Worst-case traces | section 4 (five synthetic, one real) |
| Four-vector decision | **Closed: TVC (draft 5) has one vector per used list per macroblock** (16x16 luma, 8x8 chroma, `TVC_BITSTREAM_V0.md` 6.3); no four-vector mode exists. The old MPEG-4 scope item is gone with the codec |
| Client-side load cuts | 2D reads: yes (section 6). 4-macroblock write buffer: planned, not built. Interleaved Cb/Cr, decode-ahead of 1-2 pictures, a level cap on reference words, a small reference cache: proposals from the codec review, not decided |
| Write-buffer line length | 32 words (design target) |
| † placeholders (WRR weights, queue depth, batching watermark) | set from the first untimed replay |
| I-picture burst | not an SDRAM trace (intra macroblocks read nothing); it stresses the parser and the bitstream DMA: `scenechange` covers its write-only pictures |

## 8. Regenerating
```
tools/mem_study/tvc_trace TEARS_9650.tvc tears.trace        # the first 9,650 pictures of tears_q30 (work/mem_study/tears_q30_9650.tvc)
tools/mem_study/sdram_model tears.trace --skip 9600 --frames 48 --dump X.req --treq X.treq     # [--fps 24 --mbrate 67500 --ahead 1]
tools/mem_study/gen_worst_trace.py allbi allbi.trace; tools/mem_study/sdram_model allbi.trace --fps 30 --treq allbi.treq
python3 sim/test_treq.py                                       # checks every file under work/mem_study/
```
The TVC file is `tools/tnv3_ref/py/tnv3_demux.demux('work/films/tears_q30.tnv3')` cut to its first 9,650 pictures. `--treq` needs the linear layout.

## 9. Open points for Tau Alpha
(a) Adopt or change the record layout (the contract fixes only the field list). (b) The pass criterion (section 3, revised in draft 1.1). (c) Whether `PIC_END` as a full barrier is acceptable or the harness should model the row-level overlap (it only makes the real case easier). (d) The 2D fusing rule (section 6) or an explicit 2D record in version 2 of the file.

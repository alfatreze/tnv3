# AV1-lean inter path: plan for blk v2 and steps A6-A9 (2026-10-26)

Status: plan, nothing of the inter path is built. Evidence level is stated per line: **measured** (Pocket, fit report, encoder output), **simulated**, **estimate**.
Companion documents: `docs/CODEC_AV1_INTER_PATH.md` (what an inter block needs, C0-C3), `docs/CODEC_ROADMAP_2026-10-23.md` (A6-A9), audit trail V-082, V-121, V-155, V-159.

## 1. Why inter, and what it buys

Measured on the test film (400x166, 24 fps, cq 36, lean tool set, `tools/tnv2.py encode`):

| Stream | mean bytes per frame | note |
|---|---|---|
| all-intra (`av1_intra_long`, 3 min) | 4,215 (2,632 on the 40 s loop-filter clip) | 19.7 MB for 3 min, peaks of 5.6-6.2 KB per frame where the hardware needs H33-34 ms |
| GOP 48, 6 s (`--gop 48`) | **932** (P frames **599**, key frames 10,540 / 15,666 / 23,518) | 3 key frames in 144 frames; about 4.5x smaller than all-intra |

What the encoder already does (measured, `build/hdr_dump` on that stream): **every frame has `ref_select = 0` (single reference only, no compound), `skip_mode` off, `use_ref_frame_mvs = 0` (no temporal MVs), `interp_filter` fixed (not switchable), no global motion, loop-filter levels 0.** The P frames use up to three reference slots (`ref_frame_idx` 1,0,0.. / 2,1,0..). So the first inter profile can drop compound prediction, hidden frames / reordering, temporal MV storage, switchable and dual filters, and the loop filter without losing anything the lean encoder produces. `aomenc --max-reference-frames=1` would also remove the multi-reference syntax if the area gate needs it (not yet tried).

Decode time (measured for intra, model for the rest): the intra hardware time follows about **23 ms + 1.7 ms per KB** of frame (H27 at 2.2 KB, H33-34 at 6 KB). A 23.5 KB key frame is therefore about 63 ms (1.5 frame periods), a 15.7 KB one about 50 ms. A P frame has far fewer bits but the same block-level work for every block, so its time is not known: **the first deliverable of A9 is a simulated P-frame cycle count** (the committed 10-frame trace already has nine inter frames through `tau_av1_blk`; reconstruction cycles come with C1).

## 2. Budget available now (measured, fit `nu-av160p`, DEV 241 on the Pocket)

Device: 18,480 ALMs, 308 M10K, 66 DSP. Fitted: **15,994 ALMs (87%), 277 M10K, 37 DSP**; free **2,486 ALMs, 31 M10K, 29 DSP**. Slow-0C setup slack of that fit +0.216 ns (seed 2): little timing margin to give away. Decode subsystem (V-121, nu-av1n): about 7,800 ALMs, 70 M10K, 23 DSP, of which `tau_av1_blk` 1,771 ALMs and 10 M10K (intra-only, mode-info ring 4,096 units x 21 bits).

### What inter adds (estimate, labelled)

| Item | ALMs | M10K | DSP | Basis |
|---|---|---|---|---|
| blk: intra-only 1,771 -> P-capable v2 (target total 3,000) | +1,230 | +19 | 0 | target of A6; ring 4,096 units x 58 bits (P-only: no second MV / reference: 93 - 35) = 237 kbit = 29 blocks, against 10 now |
| motion compensation, one lane (8-tap, 2 passes, line buffer) | +290 | +3 | +8 | probe `av1mc`: two lanes 581 ALMs, 6 M10K, 16 DSP, 117.9 MHz |
| reference window streamer (clamp, row bursts) + SDRAM client | +250 | +1 | 0 | estimate (the MPEG-4 row-fetch layer was of this size class) |
| reconstruction control: block prediction jobs, neighbour MVs for sub-8x8 chroma, residual order | +400 | +1 | 0 | estimate |
| register group, reference slots, buffer pointers | +100 | 0 | 0 | estimate |
| **Total** | **about 2,270** | **about 24** | **8** | |
| Free now | 2,486 | 31 | 29 | |
| **Margin** | **about 215 ALMs (9% of the free, 1% of the device)** | 7 | 21 | |

**Reading:** M10K and DSP are fine; **ALMs are the constraint**: the total would sit at about 18,260 of 18,480 (98.8%). The two-seed fits of the 160 KB builds already needed three rounds of registering to close at 87%; at 99% a placement that closes timing is not credible. The plan therefore carries an **area gate** (below) and a list of ALM sources to recover before A8.

### Where to get 600-1,000 ALMs back if the gate fails (candidates, estimates)
* **The lean tool freeze is a small pool (measured, A6b(a), V-161):** the encoder already switches palette, CfL, filter-intra, smooth, paeth, the intra edge filter, CDEF, OBMC and inter-intra off, and the hardware intra predictor has none of them. What is left is only the parsing states of `tau_av1_blk` for CfL, palette and filter-intra: removing them (probe, same wrapper) saves **72 ALMs of 1,533 (4.7%)** and 4 registers, timing unchanged (+0.25 ns at slow 0C). Worth doing when blk is touched anyway (A7-G1), not a source of hundreds of ALMs.
* `mp3_fb` 1,426 ALMs and `mp3_soc` 3,103 ALMs still carry visualiser / UI hardware that a video player does not need (V-121 table): a pruning study, synthesis only.
* The loop-filter walker stays out of the first inter profile (about 1,300 ALMs and 8-12 M10K with the line window and the transform-size map; deblocking needs 35-38 ms of hardware time per frame in simulation, which the 41.7 ms period cannot carry anyway).

## 3. Architecture decisions (proposed, for the owner)

1. **Profile 1 = single-reference P frames**, no compound, no hidden frames (`show_existing_frame` / reordering), no temporal MVs, no global motion, fixed interpolation filter, loop filter off, 64x64 superblocks, 400 wide. All of it is what the lean encoder emits today.
2. **Keep the verified intra-only `tau_av1_blk` and grow it by groups, not a from-scratch rewrite.** The roadmap said "blk v2 is a new architecture"; the intra-only blk closes 100 MHz (V-123: +0.07 ns at slow 0C in the standalone probe, 1,533 ALMs there and 1,771 in the fit) with 15 mutants killed, and the golden inter-capable blk (INTRA_ONLY = 0, 4,531 ALMs) is the executable specification (the same libaom trace bench covers its nine inter frames). The expensive parts of the golden one are the parallel MV stack (3.6 k), the variable transform tree (2.5 k) and the reference contexts (2.1 k); v2 replaces exactly those with serial, RAM-based micro-steps. Whether this stays "inside blk.sv" or becomes `tau_av1_mvstack.sv` (the scan engine as its own module, driven by blk) is decided in A6 by the budget table.
3. **Clock stays 100 MHz** (shares the SDRAM clock; blk closes it).
4. **Display / key-frame policy (A9):** hardware time for a 15-24 KB key frame is 50-63 ms; P frames are expected well under the 41.7 ms period. A **decode-ahead queue of about 4 display buffers in SDRAM** (400x166 RGB565 = 133 KB each) lets P frames build a lead that a key frame then spends; GOP 2-4 s. Seeking lands on key frames (the index flags them).
5. **Reference frames:** nine frame buffers in SDRAM (the driver already plans nine context blocks), after (no) deblocking each decoded frame is a reference; seven `ref_frame_idx` base addresses are programmed per frame.

6. **Area (decided by the owner 2026-10-26, V-164/V-165): the inter build drops the MP3 window unit** (1,309 ALMs, V-153). With inter frames the CPU has the whole hardware time to decode mono MP3; the unit only pays for itself on all-intra files. The all-intra build keeps it. Inter budget with it dropped: about 2,900 ALMs needed against 2,486 + 1,309 = 3,795 free.

## 4. Steps (replacing the A6-A9 rows of the roadmap)

Gate rule for every group: standalone 100 MHz probe within its ALM budget, libaom trace bench identical, mutants killed. Effort is in working sessions (design + tests + a 20-30 min probe each); fits are about 2 h and need the owner's go.

| Step | Work | Done when | Gate / decision |
|---|---|---|---|
| **A6** (this note) | budget table, profile 1, plan | owner decisions of section 5 | go / no-go on profile 1 |
| **A6b** (done, V-161) | measure, no RTL: (a) freeze pool: **72 ALMs**, (b) P-frame syntax: **about 0.1-0.3 M clocks** at 400x166 (scaled from the committed 800x450 stream), (c) reference limit: libaom accepts 3..7 only, `--max-reference-frames 3` costs +3% size, `--reduced-reference-set` +0.2%: **the multi-reference syntax stays** | three numbers in the audit trail | the area gate will have to be met by system pruning, not by the tool freeze |
| **A7-G1** (done, V-162) | P-only mode-info record (58 bits) and ring change (`P_ONLY`), compound / skip mode off, second MV-stack pass dropped | trace bench of a 400x166 GOP clip identical (1,903 blocks), 14 mutants killed incl. the ring record; probe at 100 MHz, RL 5, CL 7: **golden 4,946 ALMs / 381 kbit / 72.4 MHz, `P_ONLY` 4,243 ALMs / 234 kbit (29 M10K) / 77.6 MHz**, intra-only 1,533 | ring M10K as budgeted (+19); **ALMs: 4,243 against the target 3,000: the MV-stack (`cmp_i`, `nf`, `mvs`, `x_i`, `x_hitr`) and the transform-tree leaf (`tr_leaf`) paths are the whole critical set (-2.9 ns at 100 MHz): G2 and G4 must remove at least 1,243 ALMs and 2.9 ns** |
| **A7-G2 .. G4** (done in function, V-163..V-170) | the MV-stack engine, MV decode and the inter transform tree already existed in the golden blk and pass the trace bench; the work was removal and timing: compound logic removed in the profile, 44-bit stack entries, second RAM copies for the mode readers and the tree, registered addresses / contexts / constants, a register-addressed stack read port | bench identical (default 4,881 blocks, P_ONLY 1,903 blocks, 14 mutants); **probe `g2e` at 100 MHz (RL 5, CL 7): 3,284 ALMs, 1,369 registers, 225 kbit, Fmax 93.4 / 93.8 MHz, slack -0.71 / -0.66 ns, 17 ns total negative slack** (start of G1: 4,946 ALMs / 72.4 MHz / -3.8 ns) | blk 3,284 ALMs against the 3,000 target, but the budget now fits with the MP3 unit dropped (inter total about 2,950 against 3,795 free); in-situ fits have been 0.5 ns better than the probes (nu-av1n: probe -0.6, fit -0.11) |
| **A7-G3** | MV decode (`read_mv`: joint, class, fraction, high precision), precision lowering and clamping, interpolation-filter syntax (fixed) | bench identical on all MVs | |
| **A7-G4** | inter transform tree (variable transform size for inter blocks) and the inter coefficient contexts | bench identical on every inter block's transform blocks | blk total at or under 3,000 ALMs: **area gate** |
| **A7 exit** | blk v2 probe, whole decode subsystem synthesised with it | numbers vs section 2 table | **go/no-go on the area budget before A8** (freeze list, pruning study, or stop) |
| **A8-C0** | libaom oracle for the inter predictor (`AV1_INTER_TRACE` patch: per call plane position, size, MV, reference slot, filter, predicted samples) | oracle runs on the GOP clip | |
| **A8-C1** | `tau_av1_interpred` = reference window streamer + one `tau_av1_mc` lane, against the trace with a modelled reference store | predicted samples equal libaom's for all blocks (incl. sub-8x8 chroma), mutants | probe within the table |
| **A8-C2** | reconstruction control: block prediction jobs from block events (+ neighbour MVs), residual jobs from coefficient-start events | whole frames (key + inter) equal libaom's before deblocking in simulation | |
| **A8-C3** | driver and registers: reference slots, nine buffers, replay test with a two-frame then a GOP stream, fit of the first inter bitstream (two seeds), install | P frames equal libaom on a Pocket (picture + plane checksum) | owner approval for fit and card write |
| **A9** | P-frame cycles measured (simulation then Pocket), decode-ahead queue and key-frame policy in the player, GOP choice, seek | a GOP file plays at 24 fps with S0 on the Pocket; numbers in the audit trail | decides the real frame rate and GOP |

Order and dependencies: A6b can start now and overlaps A7-G1; G2-G4 are sequential (each uses the previous symbols); A8-C0/C1 can run in parallel with G3/G4 (they need only the oracle and the existing MC). Honest total (estimate): about 12-16 working sessions and three or four fits.

## 5. Owner decisions needed

1. **Profile 1** (single-reference P frames, no compound, loop filter off, 400 wide, 100 MHz): go? (recommended; it is what the encoder already produces).
2. **Grow the verified blk by groups** instead of a from-scratch rewrite (recommended, section 3.2).
3. **Area gate:** if the A7 exit table exceeds the ALM budget, which do you prefer first: freeze tools (CfL, filter-intra, edge filter, palette) or prune the system (visualiser UI hardware)?
4. **Loop filter** stays out of profile 1 (the encoder emits levels 0 already); revisit after P frames play.
5. **Resolution** stays 400x166 (800 needs about 11 more M10K and four times the decode time).

## 6. What would change this plan
* A6b(b) showing a P frame at or above about 30 ms: the decode-ahead queue and a longer GOP matter more, and 24 fps may need the inter path to run faster (a second MC lane, or the symbol decoder at 2 symbols per clock).
* (A6b(a) measured the tool freeze at 72 ALMs: system pruning (visualiser UI hardware in `mp3_fb` / `mp3_soc`, about 4,500 ALMs between them) is the main lever for the area gate; a synthesis-only pruning study is the next measurement.)
* The MV-stack engine over 1,500 ALMs even serialised: stay all-intra (the AV1 player works today at 24 fps with S0 and mono 64 kbit/s audio; files are about 4.5x larger than inter files) and put effort into the rate cap instead.

## 7. Status (2026-10-27)
Steps done in simulation: A7 (blk `P_ONLY`, 3,284 ALMs / 93 MHz probe), C0 (oracle + model), C1 (`tau_av1_interpred`), C2 (inter jobs in `tau_av1_recon_seq/_ctl/_eng`, `tau_av1_recon_buf` block-read port, `ev_bend`), C3 (reference table + REFSET command, `tau_av1_interpred2` row bursts on the shared SDRAM read client, driver `AV1P_INTER`, firmware pacing for GOP files). Whole-unit tests (`make test-rtl-av1-top-gop`, `make test-rtl-av1-replay-gop`): a 6-frame and a 12-frame lean clip decode with every frame equal to libaom, also driven by the driver's own transcript through the register block. Measured in simulation at 100 MHz: key frame 36 ms, film P frames 30-33 ms (after the burst reader; 44-59 ms before). Open: two-seed fit of `tools/tau_nu_av1_160k_inter_qsf_append.txt` (area and timing are NOT known yet), package, Pocket test, decode-ahead (A9; the coefficient parser is now the longest part of a P frame), a clip that exercises buffer reuse for the row-cache flush (no cache any more: moot), 800 wide.

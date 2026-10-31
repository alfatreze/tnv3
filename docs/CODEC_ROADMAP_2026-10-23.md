# Codec roadmap after first light (2026-10-23)

Written after V-106 (AV1 intra decode on a Pocket equals libaom pixel for pixel) and the review of the same day (V-100). Evidence level is stated per
line: **measured** (hardware or fit report), **simulated**, **estimate**.

**Update 2026-10-23 evening (owner decision):** AV1 is the main track; the MPEG-4 product branch (Branch B) is ON HOLD until the CPU relief levers
are decided (section 3, Branch B). Reason, measured on TAU V DEV 227 (V-114): MPEG-4 + mono MP3 shows only about 49% / 56% / 78% of the frames at 24 / 20 / 15 fps,
the longest frame decode is 246 ms and there are no slow SD reads, so the CPU, not the card, is the limit.

## 1. Where we stand

| Item | State | Evidence |
|---|---|---|
| AV1 intra chain (block syntax, coefficients, reconstruction, SDRAM store, RGB565 out) | correct on hardware, 400x176 key frame, loop filter off | measured, V-106: 0 of 70,400 pixels differ |
| Decode time of that frame | tile 1,952 k + colour out 315 k clocks at 100 MHz (about 23 ms) | measured |
| Deblocking walker | bit-exact in simulation, never run on hardware, 1.4-1.6 M clocks (about 15 ms) | simulated |
| Inter frames | syntax decodes, reconstruction not built | simulated / plan only |
| Timing of the integrated build | fits (308/308 M10K, 16.2 k ALMs) but slow-corner setup -1.9..-2.6 ns at 100 MHz, all in `tau_av1_blk` | measured, nu-av1k/l/m |
| `tau_av1_blk` as a design | its own probe doc calls it a golden, unoptimised implementation: 7.8 k ALMs, Fmax 72 MHz, 66 MHz was the stated target clock; MV stack 3.6 k, variable transform tree 2.5 k, reference contexts 2.1 k ALMs; "needs restructuring, not trimming" | measured, V-072/V-074 |
| AV1 all-intra playback from a TNV2 file | 480 frames at 24 fps, 0 late, decode 23 ms + read 4 ms of 41.7 ms, slowest frame 33 ms; video only, loop filter off, bitstream with negative slow-corner slack | measured, V-118 |
| MPEG-4 SP + MP3 on hardware | works technically (A/V sync, TNV1, hardware IDCT/MC/colour) but is not a good playing experience: TAU V DEV 227 shows about 49% / 56% / 78% of the frames at 24 / 20 / 15 fps (mono 64 kbit audio), longest frame decode 246 ms, S0 slow reads | measured, V-066..V-068, V-114 |
| `tau_av1_blk` after pruning the inter-only logic for intra-only builds | 1,559 ALMs (was 3,483), 94 MHz alone, -0.6 ns at 100 MHz; fit `nu-av1n` running | measured (probe), V-114 |
| Why AV1 | the lean tool set costs 0.30-0.34 of tuned MPEG-4 size at equal VMAF (about 3x) | measured, CODEC_PHASE0B |

## 2. Should the RTL be rebuilt from scratch?

**No for everything except `tau_av1_blk`.** The symbol decoder (448 ALMs), coefficient parser and inverse transform (1.96 k ALMs together), intra predictor,
reconstruction engine, frame store, colour output and registers are small, verified against libaom with mutants, and now confirmed on hardware.
Rewriting them would discard evidence for no gain.

**Yes for `tau_av1_blk`, as a new architecture (v2), not a patch.** It is the one block that is simultaneously the largest, the slowest and the one
that cannot grow into the inter product: the inter path adds motion compensation (about 0.3 k ALMs per lane, estimate from the probe), a window streamer,
nine reference buffers and a bigger deblocking map on top of a build that is already 16.2 k of 18.5 k ALMs with no MP3 window unit. **Full-product fit
with the present blk: not credible (estimate, about 17.7 k ALMs before the MP3 unit comes back).** The old probe doc already says why: wide variable-index
register arrays, eight parallel 64-bit comparators, a 300-state controller. The unit needs only about 17 M clocks per second, so a serial,
RAM-based, narrow-datapath design costs time we have and saves area we do not.

The existing test bench (libaom trace per symbol: address, alphabet, block summaries, coefficient calls) is the executable specification for v2.

## 3. Branches

### Branch A (MAIN TRACK): slim AV1, in gated steps
Goal: the full lean AV1 (intra + inter) fits next to the SoC and the MP3 path, closes timing in every corner, runs on a Pocket.

| # | Task | Done when | Gate / decision |
|---|---|---|---|
| A1 | Read the standalone 100 MHz probes (`blk-io-old` vs `blk-io-new`) | ALM and slack of the pruned intra-only blk known | if the pruned blk closes 100 MHz alone: refit; else prune the variable transform tree too (est. -2.5 k ALMs, V-074) |
| A2 | Intra-only first-light bitstream that closes every corner (and brings the MP3 window unit back if area allows) | fit report all corners positive, AV1 TEST still `OK`, picture equal | this is a stepping stone, not the product: it only needs the intra states |
| A3 | Loop filter on hardware: test stream with deblocking on, same AV1 TEST, picture compared with libaom | pixel-equal on the Pocket | measures the real deblock cost (about 15 ms) |
| A4 | More content: selectable test streams (live action, B/W, anime), a plane checksum in the test | at least 3 clips equal libaom on hardware | needs a small firmware change (file picker) |
| A5 | SoC diet to 160 KB main RAM (128 + 32 KB regions): meter remnants out of the NU image (about 22 KB), ring 24 to 16 KB | 32 M10K free, pairing check passes, VIDEO/AV rows unaffected | frees room for blk v2 and the MP3 unit |
| A6 | **blk v2 design note**: serial MV-stack engine in RAM, transform tree and reference contexts as micro-steps; budget under 3 k ALMs, target clock stated (66 or 100 MHz) | note reviewed by the owner, budget table per block | clock decision: 100 MHz needs a faster blk; 66 MHz needs a clock crossing at the SDRAM port |
| A7 | blk v2 implementation, group by group (partition and intra first, then reference, then MV stack, then transform tree), each group probed at the target clock and run against the trace bench | all 15 mutants killed, trace identical, probe within budget | stop and re-plan if a group exceeds its budget by 50% |
| A8 | Inter reconstruction C0-C3 of `docs/CODEC_AV1_INTER_PATH.md` | two-frame stream equals libaom in simulation, then on hardware | start only after A7 shows the area |
| A9 | Frame budget: key-frame policy (several frame times, decode-ahead in the nine buffers), measured P-frame cost in simulation, deblocking trade (second read port, or low/zero filter levels with measured rate cost) | numbers in the audit trail | decides the real frame rate |
| A10 | Firmware product layer on AV1 (library, resume, thumbnails) | see Branch B, shared | |

### Branch B (ON HOLD): the product on MPEG-4 SP + MP3
Status: **on hold by owner decision (2026-10-23).** The scope in `CLAUDE.md` (flat video list, thumbnails, resume, simplified OSD, Tau Omega transcoding to TNV1)
is mostly firmware and PC work and is codec-independent, but nothing built on it is worth finishing while the player cannot play at a usable rate.
Evidence (V-066..V-068, V-114): the CPU is the limit (audio MP3 about 34% of the CPU at stereo, video parse about 15.5 ms a frame, output and hooks about 5 ms; longest
frame decode 246 ms with audio interleaved; no slow SD reads).
Levers that decide whether to resume it (from V-068, estimates):
1. cheaper audio: mono MP3 (done, in 227) or IMA-ADPCM / mu-law (about 2-3% of the CPU instead of 34%; a new TNV audio codec);
2. CPU: C extension, Zbb, bigger caches (10-25% of the video part);
3. asynchronous SD reads (about 8%);
4. hardware MP3 or MPEG-4 parse (largest effect, largest RTL work, new fit).
Resume only after the owner picks levers; until then B1-B4 of the earlier plan (TAU V MPEG-4 build, library UI, Tau Omega transcoder, Preview release) are not started
beyond what exists (TAU V DEV 226/227 packages). The AV1 track does not have this problem by construction (syntax, entropy, reconstruction in hardware; CPU = audio + a driver),
which is part of why it is now the main track. The audio question is shared: both codecs still need an audio path whose CPU cost is measured.

### Branch C (optional stepping stone): all-intra AV1 video
The intra chain works now. At 15 fps the budget is 66 ms and a key frame costs about 23-36 ms (measured / simulated with deblocking), so a sustained
all-intra AV1 playback at about 12-15 fps is within reach with firmware only (loop over AV1 TEST with audio). Bitrate is worse than MPEG-4 (about
160 KB/s for the test frame size at 24 fps, estimate), so it is a hardware-validation exercise (sustained decode under SDRAM contention with audio), not a
product. Worth doing only if the contention question matters before A8.

### Branch D (not recommended): CPU-assisted syntax or a different codec
CPU-side block syntax: the CPU is 66 MHz and already spends most of a frame in the MPEG-4 parser (measured, V-045/V-046), audio also needs it.
A different codec (H.264 Baseline) is simpler than AV1 but not royalty-free for a project meant to be MIT and public. No evidence supports either.

**Update 2026-10-26:** A1-A2, A5 and the audio relief are done (V-123..V-159: the timing gap of `tau_av1_blk` and `tau_av1_recon_buf` closed, 160 KB main RAM, MP3 window unit back and used for mono audio; DEV 241 plays the 3-minute all-intra file at 24 fps with S0). A3 (loop filter) is blocked: the nu-av1 bitstreams have no walker (V-141). **A6-A9 are replanned in `docs/CODEC_AV1_INTER_PLAN.md`** (profile 1 = single-reference P frames, budget table with the fitted 2,486 ALMs / 31 M10K / 29 DSP free, blk grown by groups instead of rewritten, area gate before A8).

## 4. Order of work

1. A1 (the pruned blk, probe -0.6 ns; fit `nu-av1n` running, expected about 18:30), then A2 (an intra-only bitstream that closes every corner).
2. A3 (loop filter on hardware), A4 (more content, plane checksum) and A5 (160 KB main RAM) while blk v2 is designed (A6).
3. A7 as the long task, group by group, with the standalone 100 MHz probe as the gate for every group.
4. A8, A9 after A7, with an explicit go/no-go on the area budget before A8 starts.
5. Audio for the AV1 player (the decoder wait of about 23 ms per frame is idle CPU time: interleave MP3 decoding there, as the MPEG-4 AV test did; V-118): measure the CPU cost of the audio candidates (mono MP3 now, IMA-ADPCM, Opus) on the AV1 build before A10; this is the one item shared with Branch B.
6. Branch B stays on hold until the CPU relief levers are decided.

## 4b. Added late 2026-10-23
* TNV2 (`docs/TNV_FORMAT.md`) is the container; the library lists only `.tnv`; the Pocket's file menu offers `.tnv` (data slot 2); the player screen is full black with pause / seek / stop and a stats line (V-120). TAU V DEV 230 carries all of it and has not run on a Pocket yet.
* **800 vs 400 wide is undecided** and the build is 400 wide although the study and the frame-memory plan are 800: see V-121 for sizes, M10K and time (800 needs about 11 more M10K, four times the decode time, and a wider scanout).
* Resource table of the decode hardware as fitted vs plan: V-121.

## 5. Owner decisions needed

1. blk v2 rewrite: go (recommended), or keep trimming the current blk?
2. Target clock of the AV1 unit: 100 MHz (shares the SDRAM clock, needs a faster blk) or 66.67 MHz (needs a crossing at the SDRAM port, more relaxed timing)?
3. Branch B is on hold (decided 2026-10-23). To resume it: which CPU relief levers (cheaper audio codec, CPU extensions, asynchronous SD reads, hardware parse)?
4. Lean set freeze and 64x64-only superblocks (open since phase 0), audio codec (MP3 kept for now).
5. Target resolution: 800x334 (the study) or 400x166 (what is built)?

## 6. What would change this plan
- A1 shows the pruned blk closing 100 MHz and fitting with the MP3 unit: A7 can be scoped as an incremental restructuring, not a rewrite.
- A6's budget table shows blk v2 above 4 k ALMs: the full-product fit fails; then choose between resuming Branch B with the CPU levers (it is not a ready fallback: V-114) and keeping AV1 intra as a research result.

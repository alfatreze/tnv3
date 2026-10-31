# Handoff: novel Pocket video codec, phases 0 to 1 (2026-10-20/21): evidence for an AV1-lean design, three decoder blocks built

Read this first for the codec track, then `docs/CODEC_PHASE0B.md`, `docs/CODEC_AV1_WORKLOAD.md`, `docs/CODEC_AREA_PROBES.md`. The MPEG-4 video work on `main` (TAU DEV 205-218) is a separate track and is unaffected; owner decision 2026-10-21: the existing Tau nu fabric is NOT a constraint for the new design (budget = whole device, 18,480 ALMs / 308 M10K / 66 DSP), and purpose-built RTL beats reuse unless the existing block is shown optimal.

## Where things are
* Worktree `tau-nu/.claude/worktrees/codec-phase0`, branch `codec-phase0` (from `b108a8f`). Commits: `2e64989` (phase 0/0b study), `e05211c` (symbol decoder, V-111), `b9c18f5` (transform engine + AV1 workload, V-112). **Uncommitted: the coefficient parser, its tests, docs, V-069, this handoff, CLAUDE.md/status edits** (first action of the next session: review `git status`, commit on the owner's go-ahead). Nothing pushed.
* Audit entries V-109..V-069 (`docs/AUDIT_TRAIL.md`); the next id is V-070. DEV numbers belong to the main track (next 219).
* Git-ignored scratch in the worktree `work/`: `codec_phase0/` (clips, ivf streams, traces, results, 6.6 GB anime intermediates), `ref/aom-src` (libaom v3.15.2 checkout), `ref/aom-build` (decoder with accounting + inspection), `ref/aom-trace` + `ref/aom-build-trace` (scratch copy patched with `sim/av1/patches/decodetxb_trace.patch`), `ref/stub` (config stubs). `build/` holds generated binaries and vectors. Recreate libaom with `git clone --depth 1 --branch v3.15.2 https://aomedia.googlesource.com/aom` and cmake (flags in docs/CODEC_AV1_WORKLOAD.md and the Makefile comments; cmake and ninja were installed with Homebrew, aomenc too).
* Sources in `media/` of the MAIN checkout (git-ignored): Tears of Steel, an anime episode (DAMAGED: holes, broken seek index, silent slice corruption: clips come from one sequential decode and were checked by eye), a 1960s B&W TV episode (640x480, already a 0.7 Mbit/s H.264).

## What was established (all measured, one film + one anime episode + one TV episode)
* Equal-quality sizes against tuned MPEG-4: H.264 Main+B -39%, High -45%, HEVC -52%, SVT-AV1 -69%, libaom -73%; a lean AV1 subset (25 zero-value tools off) -70%, `lean2` -66% (still ~38% below H.264 High). Tuned ffmpeg `mpeg4` flags are 14-17% smaller than today's TNV1 flags (decoder compatibility NOT checked).
* Tools worth nothing here (<1% each): CDEF, OBMC, palette, intrabc, CfL, filter-intra, smooth/Paeth, intra edge, inter-intra, masked/distance compound, dual filter, 64-pt transforms, 128 superblock. Worth keeping: adaptive CDFs (+10-12%), rectangular transforms (+9%) and partitions (+6%), blocks >= 32x32 (+6%, 16x16 costs +16%), hierarchical B with future references (+32-42%), deblocking. Half-resolution coding and film-grain synthesis gave nothing.
* Real streams (libaom, cq 44): coefficients 58-76% of bits; 109-528 k symbols/s mean, worst second 0.81 M, worst frame 184 k; bi-prediction 52-71% of inter area; OBMC 0%; skip 73-97%; vertical MV p99 3-55 px.
* Blocks (all libaom-verified, mutation-tested, standalone-probed; method `tools/vm_probe.py`): `tau_av1_symdec` 448 ALMs, 28 clk/symbol; `tau_av1_itx` 848 ALMs, 2 DSP, 132 MHz; `tau_av1_coef` 666 ALMs, 98 MHz (-0.79 ns at 100), 6 M10K, 5 DSP, end to end 32.7 clk/symbol with the RTL range decoder. Together 1,962 ALMs (10.6%), ~32 M10K. Nothing has run on a Pocket.

## Not done (in order)
1. Block-level syntax (partition, modes, MV coding and reference-MV stack, contexts for the coefficient parser, tx_type, delta-q), frame level (headers, tile/frame CDF contexts, reference management).
2. 8-tap sub-pel motion compensation for two references with reference fetch (the memory system, not the arithmetic, is the question).
3. Directional intra predictor, deblocking filter; then integration (decode-ahead queue, decoder clocking, memory map, display path) and a fit.
4. Decisions pending: standard AV1 syntax vs a transcoded simpler syntax (needs the syntax bit-share split, already available from accounting), which `lean` set to freeze, audio (Opus vs MP3 listening pack `work/codec_phase0/audio/`, never listened to), vertical-range limit (no AV1 encoder has one).

## Gotchas
* The worktree-isolated shell guard refuses commands it cannot verify (heredocs piping into python, loops over variables in a compound command, `cd` into other directories changes the working directory for later calls): write scripts with the editor tools, run plain commands from the worktree root, use absolute paths.
* Probes: a compile takes 11-14 minutes (state the estimate and finish time when launching, per project rules); results via `tools/vm_probe.py status NAME`, critical path via `vm_probe.paths(NAME)`.
* Simulation: RAMs power up as X in Icarus (a product with a zero weight is still X): testbenches clear the memories by hierarchical reference.
* Equivalent mutants happen (a bool probability truncated to 10 bits, a dead clamp): replace them with distinguishable ones, do not count them.
* The owner approves downloads/installs explicitly; the owner said libaom and other test tools may be installed. SD card writes need approval per install (not touched in this track).
* Provenance: libaom is a test oracle only; `tau_av1_itx_rom.svh` is generated from libaom's butterfly structure with its BSD-2 notice (docs/PROVENANCE.md).

# Format and codec research

The current format snapshots are [TVC](TVC_BITSTREAM_V0.md), [TAC](TAC_BITSTREAM_V0.md), [TNV3](TNV3_CONTAINER_V0.md), and [TST](TST_SUBTITLES_V0.md). Tau V retains normative format authority until that is explicitly transferred.

This directory also preserves historical work, which may describe older drafts, experiments, proposed designs, or results that have since been superseded. Refer to the specification snapshots and release validation for current behavior.

- `ENCODER_LOG.md`, `ENCODER_TOTAL_RESULTS.md`, `ENCODER_PARALLEL_HYPOTHESES.md` and `encoder_*.json` contain encoder experiments and measurements.
- `CODEC_*.md` and `codec_phase0/` cover codec alternatives, AV1 research, video quality studies and measurement data.
- `TVC_*.md`, `TAC_*.md`, `TNV3_*.md`, `TST_*.md` and `VIDEO_*.md` contain format, implementation, player and hardware research.
- `handoffs/` contains selected historical codec and video session records; `research/` contains decode-quality research.
- `tools/codec_study/` and the research scripts under `tools/tvc_ref/` and `tools/` are archival experiment tools. They may depend on external software, old repository layouts, source clips, intermediate artifacts or git tags that are not included. Local machine paths were replaced with placeholders. Their presence does not imply that every historical experiment is reproducible from this release alone.

Media-derived clips and conformance vectors are excluded from the public repository and packages. Host, simulation and Pocket results in the historical records retain the evidence level stated there; inclusion does not constitute a new hardware test. The Rust crate remains planned.

## Research by track

- **MPEG-4 Part 2 stream and player:** [VIDEO_DECODER_DESIGN.md](VIDEO_DECODER_DESIGN.md) and [VIDEO_PROJECT_START.md](VIDEO_PROJECT_START.md) record the earlier player design. Player implementation and hardware validation remain in Tau V.
- **AV1 subset exploration:** [CODEC_AV1_BLOCK_SYNTAX.md](CODEC_AV1_BLOCK_SYNTAX.md), [CODEC_AV1_INTER_PATH.md](CODEC_AV1_INTER_PATH.md), [CODEC_AV1_INTER_PLAN.md](CODEC_AV1_INTER_PLAN.md), [CODEC_AV1_RECON_MEMORY.md](CODEC_AV1_RECON_MEMORY.md), and [CODEC_AV1_WORKLOAD.md](CODEC_AV1_WORKLOAD.md) concern AV1 models, implementation studies and workloads.
- **Custom TVC, TAC, TNV3 and TST:** use the four specification snapshots above, [TNV3_CODEC_SCOPE_2026-10-27.md](TNV3_CODEC_SCOPE_2026-10-27.md), format-prefixed documents, and `ENCODER_*` reports. These describe the newer custom format family and its tools.
- **General and comparative research:** [CODEC_PHASE0.md](CODEC_PHASE0.md), [CODEC_PHASE0B.md](CODEC_PHASE0B.md), [CODEC_ALTERNATIVES_2026-10-27.md](CODEC_ALTERNATIVES_2026-10-27.md), [research/DECODE_QUALITY_RESEARCH.md](research/DECODE_QUALITY_RESEARCH.md), and `codec_phase0/` contain cross-format studies and historical measurements.

The classifications aid navigation; some studies compare several tracks. Consult each document's date and evidence level rather than treating an experiment as validation of another format.

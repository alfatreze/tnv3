# Format and codec research

The current format snapshots are [TVC](TVC_BITSTREAM_V0.md), [TAC](TAC_BITSTREAM_V0.md), [TNV3](TNV3_CONTAINER_V0.md), and [TST](TST_SUBTITLES_V0.md). Tau V retains normative format authority until that is explicitly transferred.

This directory also preserves historical work, which may describe older drafts, experiments, proposed designs, or results that have since been superseded. Refer to the specification snapshots and release validation for current behavior.

- `ENCODER_LOG.md`, `ENCODER_TOTAL_RESULTS.md`, `ENCODER_PARALLEL_HYPOTHESES.md` and `encoder_*.json` contain encoder experiments and measurements.
- `CODEC_*.md` and `codec_phase0/` cover codec alternatives, AV1 research, video quality studies and measurement data.
- `TVC_*.md`, `TAC_*.md`, `TNV3_*.md`, `TST_*.md` and `VIDEO_*.md` contain format, implementation, player and hardware research.
- `handoffs/` contains selected historical codec and video session records; `research/` contains decode-quality research.
- `tools/codec_study/` and the research scripts under `tools/tvc_ref/` and `tools/` are archival experiment tools. They may depend on external software, old repository layouts, source clips, intermediate artifacts or git tags that are not included. Local machine paths were replaced with placeholders. Their presence does not imply that every historical experiment is reproducible from this release alone.

Media-derived clips and conformance vectors are excluded from the public repository and packages. Host, simulation and Pocket results in the historical records retain the evidence level stated there; inclusion does not constitute a new hardware test. The Rust crate remains planned.

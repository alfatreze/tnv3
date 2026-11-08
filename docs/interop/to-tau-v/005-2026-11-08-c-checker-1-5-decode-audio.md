# 005 · TNV3 codecs → Tau V · 2026-11-08 · C checker at draft 1.5/1.6, TAC decode via C tools

**Replies to:** none. Basis: repo at `ba878bf` plus uncommitted changes; Tau V commit `f930a740` (pins verified unchanged).

## Facts
- **F-1 (C checker).** `tools/tnv3_ref/tnv3.c` now checks draft 1.5 and 1.6 (outline width, baked outline plane equal to the dilation of the fill plane, outline padding, event bottom at most 720, codec id byte 22). `tnv3 check` says `valid` on `tensei06_400_subs2.tnv3` and `tensei06_subs2.tnv3` (read in place). Together with the Rust checker (message 003) two independent implementations now agree on both films. Neither decodes TVC or TAC payloads in `check`.
- **F-2 (TAC vectors pinned, in place).** `tools/tac_ref/vectors/*` (t01-t08, reference WAVs, `fx_expected.txt`, manifest) are in `tests/pins/tau_v_pins.json` by SHA-256 (all matched at `f930a740`). Our C `tac dec` reproduces every reference WAV exactly; `tac_fx dec` is within 1 sample (t08: 3, RMS 0.107), matching your spec figures.
- **F-3 (audio decode).** New `tau-codecs-cli decode-audio IN.tnv3 OUT.wav [--model integer|reference]` wraps `tnv3 demux` and the C TAC decoders (not an independent decoder). On `tensei06_400_subs2.tnv3`: 68,276,000 samples per channel (matches `audio_samples`), integer vs reference max difference 1, RMS 0.220. The pure-Rust TAC decoder is parked until the format is closed.

Open for you: nothing new. Open for us: Rust subtitle reader/mux, authoring design, pinning the older TNV3/TVC vectors.

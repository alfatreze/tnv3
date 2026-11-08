# 007 · TNV3 codecs → Tau V · 2026-11-08 · draft 1.5/1.6 vectors as files, Rust subtitle reader

**Replies to:** none (follows our 006).

## Facts
- **F-1 (vectors, for you to pin if you want).** `tests/vectors/tnv3_15_16/` in our repository: 18 files (6 valid `ok*`, 12 must-reject `bad*`), 76 kB, listed with SHA-256 and the broken rule in `expected.txt`. They cover outline width 0, 1+2, 4 and 5 (reject), outline plane not equal to the dilation, a set padding bit in an outline row, variant reserved byte, missing plane, table one byte short, event bottom exactly on row 720 (valid) and one row below (reject, one line and with a break), codec id 0 / 1 / 255, header byte 23, and codec id 1 without subtitles. Expected verdicts come from the spec; our C checker (`tnv3 check`) and our Rust checker both agree on all 18. Generator: `tests/vectors/make_vectors_15_16.py` (stdlib only, deterministic, no data of yours; the video is a synthetic gradient through our own `tvc_enc` and `tnv3`). Please reference them by path and SHA-256 as usual, not copy.
- **F-2 (reader).** The Rust crate now reads the subtitle area into data (`tau_video::subtitle_read`: tracks, palettes, variants, events, glyph fill and outline planes) and has a reference drawing of section 6.3 (text coverage with palette index, outline layer, later glyph wins on equal alpha). `tau-codecs-cli subtitles FILE.tnv3 [--json] [--events] [--render SECONDS OUT.ppm]` prints tracks and events or draws one picture. It applies none of the player parameters of 6.4. Fuzz-style test: flipping every byte of an area never panics.
- **F-3 (what this may help you with).** `--render` gives a second drawing of an area independent of `tst_render.py`; we have not compared the two pixel for pixel.

Open for us: ASS styling when extracting from a source file, comparison of our drawing with `tst_render.py`, the mux in pure Rust.
Open for you: R-4 (clamp case), the large decoder configuration decision, the 400 film's device-side failures, R-3.

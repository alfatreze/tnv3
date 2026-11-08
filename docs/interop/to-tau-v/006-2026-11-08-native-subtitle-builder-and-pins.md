# 006 · TNV3 codecs → Tau V · 2026-11-08 · pure-Rust subtitle builder, n22 pinned

**Replies to:** `to-tnv3/004`, `005`, `006` (read in place).

## Facts
- **F-1 (n22, pins).** Your `n22_video_codec.tnv3` (31 vectors) is pinned by SHA-256 together with the changed `expected.txt`; our pin check had flagged `expected.txt` as changed, as designed. Both our checkers reject it (`tnv3 check`: "unsupported video codec id"; Rust `container-preview`: `E-TNV3-032`), and the Rust conformance run passes all 31. Your tree state is uncommitted for those files, so the pins say so; we will re-pin when you commit.
- **F-2 (subtitle authoring, pure Rust).** The owner wants authoring without Python. `tau_video::subtitle` (crate feature `subtitle-builder`) is a Rust port of `tst_build.py` (tracks SRT/ASS, fonts per family and style with `@SCALE`, speaker styles, italic fallback colour, palette of 16, SDH and forced flags, wrapping, upward stacking, centring of a lone line, bar/picture region, baked outline per size, area packing, own validator), using `ab_glyph` instead of Pillow. Compared with your tool on `sample.srt`, `sample.ass` and `demon.ass` (Roboto, Roboto Italic, UnifrakturCook; 16x24/20x28/26x36; outline 2): **event lists are byte-identical, chosen font sizes and every glyph advance are identical, areas have identical sizes**; only glyph pixels differ (unhinted outlines), by at most one grey level in 176,600 compared pixels (mean 0.008 on a 0..3 scale), none by two or more. We did not port your clamp: when a stack does not fit above row 0 it warns with your exact text and clamps like your tool (a test covers it). Two approximations remain: HTML named entities are a short list, and character categories for dropping are approximate.
- **F-3 (what you can run).** `tau-codecs-cli build-subtitles OUT.bin --video WxH --track FILE[:lang[:default][:forced]] ... --font ... --sizes ...` (native by default; `--builder PATH` still runs your Python tool for comparison), and `encode --subtitle-track N | --subtitle-file F` which authors from the source and muxes in one pass. A source with two audio and two text subtitle tracks, encoded with `--audio-track 1 --subtitle-track 1`, gave a file that `tnv3 check` calls valid. Your `fonts/Roboto-Regular.ttf` and `Roboto-Italic.ttf` are pinned in place (we copy nothing; a default font is still an owner decision).
- **F-4 (800).** Understood (your 005 F-1 and 006): 800 stays an encoder option; the format is fine, the limit is your decoder build.

## Requests
- **R-4 (clamp).** Still open on your side: what the spec should say when the stack does not fit above row 0 (split, drop, or clamp). We follow your tool until you decide.

Open for us: Rust subtitle reader/mux, ASS styling when extracting from a source file (SRT only today), pinned 1.5/1.6 must-reject vectors.
Open for you: R-4, the large decoder configuration decision, the 400 film's device-side failures, R-3.

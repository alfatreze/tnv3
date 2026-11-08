# 002 · TNV3 codecs → Tau V · 2026-11-08 · draft 1.5 adopted, width and level added

**Replies to:** own 001, and Tau V 001/002 (R-002 to R-006). Owner decisions of 2026-11-08 are applied. Basis: repo at `ba878bf` plus uncommitted changes (nothing committed yet); no hashes of ours to pin yet.

## Facts
- **F-1 (draft 1.5 in spec and Rust checker).** `docs/TNV3_CONTAINER_V0.md` is now draft 1.5: variant `outline_w` 0..4 plus reserved byte 0; per-glyph outline plane `(cell_h + 2w) x ceil((cell_w + 2w)/4)` bytes; layer order picture, box, outline, text; outline width removed as a player setting; outline plane must equal the unclipped disc dilation (`dx^2 + dy^2 <= w^2`) of the fill plane; padding bits 0. `outline_w = 0` is byte-identical to 1.4. Read from your `docs/TST_OUTLINE_BAKED_V1_5.md` in place.
- **F-2 (checks).** The Rust checker adds: `outline_w <= 4`, reserved byte 0, glyph record length with the plane, plane equals dilation, outline padding bits, and for every event `y + lines * cell_h <= 720` (lines = 1 + number of 0xFFFF). Tests: valid area plus four rejected mutations (wrong plane, padding bit, bottom bound, `outline_w` 5).
- **F-3 (your film).** `tau-codecs-cli check tensei06_400_subs2.tnv3 --scope subtitle-structure` (built from the working tree, crate 0.3.0) reports no findings: 1 track, 4 variants, 2533 events, 400 glyphs. Only that file was checked, only that scope; the 800-wide film was not run. The old `E-TNV3-SUB` at +60 is gone.
- **F-4 (F-006, F-007, area size in the spec).** F-006 is spec 6.5 (recommended, not checked); F-007 is in 6.4 (normative, effective offset `min(voffset, 720 - maxbottom)`); the 1 MB area size is recorded in 6.1 as your player profile, not a format limit.
- **F-5 (CLI `--width` and `--level`).** `encode ... --width W` (even, 16..800; the box is W x 9/10 W rounded to even, so 400 gives 400x360 and 800 gives 800x720) and `--level 0..3` (0 = smallest that fits). Defaults unchanged: 400 and automatic, so 800 is opt-in only. Both encode paths use them; segments shrink for wide pictures (120 frames at 800). Not run on a real film yet; unit-tested for parsing, validation and reported settings only. `probe` estimates still assume 400x360.
- **F-6 (not done).** Our C reference `tools/tnv3_ref/tnv3.c` is still draft 1.4 and rejects your areas; there are no 1.5 vectors with SHA-256 yet; authoring (subtitle builder in the CLI/crate, built-in or external subs) is decided but not designed or started.

## Questions and requests
- **Q-1 (C-2 repeated).** Do you need CLI checks for wide pictures (800x450, level 2)? The video level checks exist; no film of that size has been checked here.
- **Q-2 (C-3 repeated).** Which `tau-codecs-cli` build did you run (your F-003 says 0.4.0-test.1; this tree's manifest says 0.3.0)?
- **R-1.** For the 1.5 vectors: point us at the golden cases you consider authoritative (your `tools/tst_ref/test_tst.py`, `tst_golden.py`) and a small area to pin by path and SHA-256; we will not copy them.
- **R-2.** For authoring, tell us which parts of `tst_build.py` are authoritative beyond the spec (wrapping rule, stacking, clamp behaviour for the 15:32 event) so a port can match it.

Open for you: Q-1, Q-2, R-1, R-2.
Open for us: C reference checker to 1.5, 1.5 vectors, authoring design.

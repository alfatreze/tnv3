# TST subtitles v0: reference authoring and rendering tools (updated 2026-10-28 for container draft 1.4)

TST is the subtitle part of the TNV3 container (`docs/TNV3_CONTAINER_V0.md` section 6, draft 1.4): one **subtitle area** holding tracks (language, `default`, `forced_track`, a 16-entry RGB **palette**), each with 1..8 size variants (event list + glyph table of 2-bit alpha, own `cell_w x cell_h`). Events carry a start colour (palette index 0..15) and a `flags` byte (bit 0 `forced`, bit 1 `sdh`); glyph references `0xFFE0 + k` switch the current colour to palette entry k, `0xFFFF` is a line break. The player's display parameters (6.4) are not stored; the renderer here implements them so every value can be tried. This document covers the tools and the choices made where the container text is open.

## Tools (`tools/tst_ref/`, Python 3 + Pillow only)
- `tst_area.py`: `pack_area(tracks)` / `unpack_area(bytes)` (section 6.1 layout, with `palette`).
- `tst_build.py --track FILE[:lang[:default][:forced]] ... --sizes 16x24,20x28,26x36 --font [NAME[:STYLE]=]FILE[@SCALE] ... [--styles styles.json] --video WxH -o AREA.bin`
  - `--track` is repeatable (SRT or ASS). `lang` is an ISO 639 code (default `und`); `default` marks the default track; `forced` makes it a forced-only track (track flag set **and** the `forced` flag on all its events).
  - `--sizes`: cell sizes of the variants, strictly increasing height. **Every variant has its own rasterisation and its own layout** (font size, wrapping, event records, placement).
  - `--font`: see Fonts below. `--styles`: speaker prefixes, see below. `--italic-color` (default `FFE040`) is the colour of italics when the family has no italic face.
  - `sdh` flag: set automatically on lines that are entirely a bracketed sound description (`[...]`, `(...)`, several groups, optional leading `- `) or only music notes; `--no-sdh` switches it off. A two-line event with one bracketed line and one dialogue line gets two records, only the first flagged.
  - Also: `--fps N/D`, `--align center|block`, `--placement auto|bar|picture`, `--margin`, `--bottom`, `--font-size` (base size, single size only; default is the largest that fits the cell).
  - The area is validated with the container's Python checker (`tnv3_demux.check_area`) before it is written. Mux: `tnv3 mux VIDEO.tvc -s AREA.bin -o OUT.tnv3` (TVC0 files need `-L`), check: `tnv3 check OUT.tnv3`.
- `tst_render.py FILE.tnv3|AREA.bin [--video WxH] --time S -o OUT.png [--frame IMG] [--panel] [--preset NAME] [parameters]`: the state at time S on the 800x720 window. Without `--frame` the picture is grey on black bars; with it the image is placed as the picture, centred (resized to the video size if it differs). `--panel` writes the 1600x1440 nearest-neighbour version. A `.tnv3` supplies its own picture size; an area file needs `--video`.
  `--sheet OUT.png [--presets a,b,c]` renders the same moment once per preset (all by default), labelled, two per row, each cell cropped to the subtitle band plus 40 rows (`--panel` for 2x).
- `presets.json`: named parameter sets. Parameters resolve as defaults, then the preset, then command-line values.
- `demo_demon.py OUT_DIR [FRAME.png] [--video WxH] [--demon FILE.ttf]`: builds `samples/demon.ass` with Roboto, Roboto-Italic and a demon font and renders per-moment images, `sheet_demon.png` and `panel_demon.png`. The demon font is the single `--font Demon=...@1.15` line in the script (or `--demon`).
- `test_tst.py`: build, mux with a TVC vector, `tnv3 check`, Python parse, area and palette round trips, flags, per-size layout, fonts and colours, each renderer parameter checked on pixels, every preset rendered, refusals. 0 failures.
- `samples/`: `sample.srt`, `sample.ass`, `sample_sdh.srt`, `sample_forced.srt`, `demon.ass` (default font, a demon style in red, an inline red word, italics, SDH, an inline font+colour run, several colours), `demon.srt` and `demon_styles.json` (speaker prefix `DEMON:` and `<font>` tags).

## Fonts and licences (`fonts/`)
| File | Use | Licence |
|---|---|---|
| `Roboto-Regular.ttf`, `Roboto-Italic.ttf` | default font and its italic | SIL OFL 1.1, `OFL-Roboto.txt` (copied from `~/Downloads/FONTS/Roboto/static`) |
| `UnifrakturCook-Bold.ttf` | the "demon" fantasy blackletter font of the demo | SIL OFL 1.1, `OFL-UnifrakturCook.txt` (Google Fonts, added by the owner) |

Roboto and UnifrakturCook cover Latin; Roboto also Greek and Cyrillic. No music-note glyph in either: a note in text becomes `?` (the note-only SDH test uses the string directly).

## Fonts: how they are used
- `--font [NAME[:STYLE]=]FILE[@SCALE]`, repeatable. The **first** `--font` is the default font and must be a regular face. `NAME` is the family name (case-insensitive; `default` when omitted); `STYLE` is `regular` (default), `italic`, `bold` or `bolditalic`: the face used when the text asks for that style. `@SCALE` (0.3..3) scales that face's size (an italic or bold face without `@` inherits the regular face's scale). Example: `--font Roboto=Roboto-Regular.ttf --font Roboto:italic=Roboto-Italic.ttf --font Demon=UnifrakturCook-Bold.ttf@1.15`.
- Every (face, character) pair used becomes **its own glyph**; a character used in two faces has two glyph ids. All faces share the table's cell and one baseline: the base size is the largest at which every face (at its scale) has `ascent + descent <= cell_h` and every used glyph fits `cell_w`; the baseline is placed from the largest ascent and descent over the faces.
- Missing styles: a missing bold face is ignored; a missing italic face falls back to the **italic colour** (old behaviour, `--italic-color`, authored into the palette like any colour). With an italic face no colour is involved.
- Characters missing in a face become `?` of that face (warning).

## Authored colours and palette
- **ASS:** styles are read (`Fontname`, `PrimaryColour` `&H[AA]BBGGRR`, `Bold`, `Italic`); each event uses its `Style` (unknown style: `Default`, else the first). Style `Fontname` maps to a `--font` family (unknown names use the default font, with a warning). Inline `{\fnNAME}`, `{\c&H..&}` / `{\1c&H..&}`, `{\i0/1}`, `{\b0/1/weight}`, `{\r}` / `{\rStyle}` become font runs and colour switches; `{\c}`, `{\fn}`, `{\i}`, `{\b}` alone return to the style's value. `{\p1}` drawings are dropped; positioning, alpha, blur, karaoke etc. are ignored.
- **SRT:** `<font color="#rrggbb" face="NAME">...</font>` (colour names red, yellow, ... also accepted), `<i>`, `<b>`; other tags dropped.
- **Speaker styles (`--styles styles.json`):** `{"DEMON:": {"font": "Demon", "color": "FF2020"}}` (also `bold`, `italic`). A source line that starts with the prefix (after an optional `- `) loses the prefix and one following space, and the rest of that line gets the font and (where it has no authored colour of its own) the colour.
- **Palette per track:** entry 0 = the default style's colour (ASS: style `Default` or the first style; SRT: white). Colours equal to it use index 0. Other colours get entries 1..15 in **order of first use** over the track. A 16th distinct colour and beyond maps to the nearest existing entry (euclidean RGB) with a warning. Unused entries are grey `A0A0A0`.
- **Switches inside events:** a line is one event (one centred x) with `0xFFE0 + k` switches wherever the colour changes (spaces never trigger a switch). The event's start colour is that of its first glyph. In `--align block` a multi-line event keeps the current colour across line breaks, as the container specifies.

## Display parameters (renderer, section 6.4)
| Parameter | Option (JSON key) | Default | Meaning |
|---|---|---|---|
| track | `--track` (`track`) | default track, else track 0 | language code, index, or `off` |
| size variant | `--size` (`size`) | middle variant | index (negative = from the largest) or `WxH` |
| subtitles | `--subs on\|off` (`subs`) | on | off shows only forced events |
| hide SDH | `--hide-sdh` (`hide_sdh`) | shown | skip events with the `sdh` flag |
| delay | `--delay MS` (`delay`) | 0 | positive = later |
| vertical offset | `--voffset ROWS` (`voffset`) | 0 | added to every event's y |
| text colour | `--text-color RRGGBB` (`text_color`; `--color0` is an alias) | track entry 0 | replaces palette entry 0 only |
| override all colours | `--override-colors` (`override_colors`) | off | every entry drawn in the text colour |
| outline | `--outline-color`, `--outline-width 0..4` | black, 0 | width 0 = none |
| box | `--box-color RRGGBB`, `--box-pad 0..16` | none, 4 | box only when a colour is given; solid |

No opacity, blur or shadow. Layers, bottom to top: picture, box, outline, text. A layer pixel with coverage c (0..3) is blended as `out = (below * (3 - c) + colour * c + 1) / 3` (integer); the box has coverage 3 inside. `--color1..3` no longer exist: those colours are the track's authored palette entries.

## Presets (`presets.json`)
| Name | What |
|---|---|
| `classic` | text colour white (entry 0), 2 px black outline |
| `thin_outline` | white, 1 px black outline |
| `yellow` | entry 0 yellow, 2 px black outline |
| `box` | white on a solid black box, padding 4 |
| `yellow_box` | yellow on a solid dark-blue box (padding 6) with a 1 px black outline |
| `large` | largest variant, white, 3 px outline |
| `small_plain` | smallest variant, light grey, no outline |
| `high_contrast` | cyan, 4 px black outline, black box padding 10 |

A preset's `text_color` replaces entry 0 even if the source gave its default style another colour; authored colours (the red demon) are kept. Add `"override_colors": true` to a preset to force one colour.

## Choices where section 6.4 / 6.3 is open
1. **Delay sign:** `p = t - delay`, a positive delay shows subtitles later.
2. **Subtitles off:** the forced track of the same language as the selected (or default) track, at the variant with the nearest cell height; if none, the selected track itself (its `forced`-flagged events still show). With subtitles on only the selected track is drawn and its `forced` events are ordinary. `--track off` equals `--subs off`.
3. **Track by language:** the first non-forced track with that code (else the first with it). Default size: the middle variant; an out-of-range size is refused.
4. **Outline mask:** grey-scale dilation (maximum) of the 0..3 coverage over `dx^2 + dy^2 <= w^2` (5, 13, 49 pixels for w = 1, 2, 4), clipped to the grid; the outline has one colour whatever the text colour.
5. **Overlap:** larger alpha wins; on a tie the glyph drawn later (later in the event, later event) wins, as 6.3 now says.
6. **Box rectangle:** per active event, from the origin to the pen extent of its longest line (sum of advances) and `lines * cell_h`, grown by the padding; several events' boxes are one solid union (stepped edge when line widths differ). Colour switches do not move the pen and do not affect the box.
7. **Vertical offset** also moves the box; drawing outside the grid is clipped.
8. **Picture:** `--frame` is resized to the video size (Lanczos) when needed.
9. **Italic fallback** is an authored colour (palette entry) rather than a fixed class; the player's text-colour setting therefore does not recolour it (only `--override-colors` does).
10. **Faces and the table:** separate glyphs per face even where the bitmap would be equal (for example the space); no deduplication is attempted.

## Input handling (encoder)
Text is NFC-normalised, whitespace collapsed, empty lines removed; events with no visible text are dropped (counted). Times are exact: `start = ms * 48` (or snapped with `--fps`); `end <= start` becomes one tick later; events sorted by start. SRT: index lines ignored, UTF-8 (BOM allowed), HTML entities decoded. ASS: `\N` hard break, `\n`, `\h` spaces.

## Glyph table
One glyph per used (face, character), sorted by (face in `--font` order, code point); glyph id = index. Rasterised at 4x supersampling on the cell with the common baseline on a whole grid pixel, box-downsampled, `round(a * 3 / 255)`. Advance `round(font advance)` clamped to 1..cell_w, no kerning.

## Layout rules (encoder, per variant)
1. Stepping as in 6.3. Line pitch `cell_h`. Greedy wrapping by advance sums at `800 - 2 * margin` = 720 (the original space glyph between two words is kept, whatever its face); an over-wide word is split by characters; source hard breaks are kept.
2. `center` (default): one event per line, `x = (800 - width) / 2`, colours by switches. `block`: lines with equal `sdh` become one event with line breaks.
3. Region per variant: rows needed = tallest **stack of simultaneously active events** times `cell_h` + bottom margin (24) + 8; fits the bottom bar: text goes there, else over the picture, last line 24 rows above the picture bottom.
4. Overlapping events are stacked upwards, the later above the active ones.

## Measured sizes (800x334, 3 variants 16x24 / 20x28 / 26x36)
| input | glyphs | glyph tables | event lists | region |
|---|---|---|---|---|
| sample.srt, Roboto (1 face) | 71 | 6,891 / 10,015 / 17,967 B | 1,186 / 1,202 / 1,216 B (13/14/15 records) | bar, bar, picture |
| sample.ass, Roboto | 45 | 4,369 / 6,349 / 11,389 B | 564 B each (8 records) | bar |
| demon.ass, Roboto + Italic + UnifrakturCook (3 faces) | 77 | 7,473 / 10,861 / 19,485 B | 676 B each (8 records, 9 switches) | bar |

Area sizes: sample.srt alone **38,603 B**, sample.srt + sample.ass **62,525 B**, demon.ass alone **39,973 B**. The palette costs 48 B per track. Per glyph: 97 / 141 / 253 B. Merging colour runs into one event saved records (sample.srt 16 -> 14 at 20x28). **Estimate for a film with 1,000 events** (label: estimate): about 100 kB of event list per variant and about 100 glyphs per face (more faces add glyphs: each additional face used for only a few words adds those glyphs at 97 / 141 / 253 B), so about 350 kB for one language with three sizes.

## Open points
- Spec 6.4 text is the owner's; the tools follow it. A real fantasy font only needs another `--font` argument.
- No kerning and no shaping (bidi, Arabic, Indic, CJK need work and wider cells).
- Fonts are applied per ASS style and per override; `\fs` (size) and `\fscx` are ignored, use `@SCALE` per face.
- ASS positioning (`\an8`, `\pos`) is ignored.
- A font that lacks a character used with a different face never borrows it from another face; it becomes `?`.

# TNV3 container v0 (draft 1.4, 2026-10-28): normative specification

TNV3 carries one TVC video stream (`docs/TVC_BITSTREAM_V0.md`), at most one TAC audio stream (`docs/TAC_BITSTREAM_V0.md`) and optional subtitle tracks, each with one or more sizes of baked glyphs. This document is the only normative description: an independent demuxer must be implementable from it alone. Reference: `tools/tnv3_ref/tnv3.c` (`mux`, `demux`, `check`, `info`). Conformance: `tools/tnv3_ref/vectors/` (9 valid + 15 must-reject files, `expected.txt`, built by `make_vectors.py` from the TVC and TAC vectors), `check_roundtrip.py` (demuxed streams decode like their sources), `mutants.py` (10 checker mutants, all killed), independent Python demuxer `tools/tnv3_ref/py/`.
Build and test from the worktree root: `cc -O2 -Wall -o tools/tnv3_ref/tnv3 tools/tnv3_ref/tnv3.c`; `python3 tools/tnv3_ref/make_vectors.py`; `python3 tools/tnv3_ref/check_roundtrip.py`; `python3 tools/tnv3_ref/mutants.py`; `python3 tools/tnv3_ref/py/run_vectors.py`.
Draft 1.4 gives every subtitle track an authored 16-colour palette and lets colour change inside an event (any number of fonts was already possible: glyphs are bitmaps). Draft 1.3 replaced the single subtitle block and glyph table by a subtitle area with tracks and size variants, and gives events a flags byte (section 6); the player's adjustable display parameters are listed in 6.4. Draft 1.2 added the chunk byte rate (section 4.2). Draft 1.1 added the clarifications of section 7b. Draft 1 replaced draft 0 (same day): exact block layout, the seek audio frame in the index instead of forcing TAC resets at key chunks (existing TAC streams reset every 16 frames), and the precise definition of D(n).

Conventions: all integers little endian and unsigned; offsets are from the start of the file; `align4(x) = (x + 3) & ~3`; `floor` division; `CRC32` = the IEEE 802.3 CRC (reflected polynomial 0xEDB88320, initial value 0xFFFFFFFF, final XOR 0xFFFFFFFF; CRC32 of the ASCII bytes "123456789" = 0xCBF43926).

## 1. Layout
In this order, with nothing else in the file:
1. header, 64 bytes at offset 0;
2. `frame_count` chunks (section 4), the first at offset 64, each next one at `align4` of the end of the previous one; the padding bytes are 0;
3. the index (section 5) at `index_offset = align4(end of the last chunk)`, `16 * frame_count` bytes;
4. if flag `subs`: the subtitle area (section 6) at `sub_offset = index_offset + 16 * frame_count`, `sub_length` bytes;
5. the file ends exactly at the end of the last block present (no trailing padding).

## 2. Header (64 bytes)
| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `TNV3` |
| 4 | 2 | version = 0 |
| 6 | 2 | flags: bit 0 `crc` (every chunk carries a CRC32); bit 1 `audio`; bit 2 `subs`; other bits 0 |
| 8, 10 | 2 + 2 | width, height of the output picture (1..65535 each), as in TVC |
| 12, 16 | 4 + 4 | `fps_num`, `fps_den` (both 1..65535; e.g. 24000/1001) |
| 20 | 1 | TVC level, 1..3 (TVC section 8) |
| 21 | 1 | audio channels: 1 or 2 if `audio`, else 0 |
| 22 | 2 | reserved, 0 |
| 24 | 4 | audio sample rate: 48000 if `audio`, else 0 |
| 28 | 4 | `frame_count`: number of pictures = number of chunks, at least 1 |
| 32 | 4 | `audio_samples`: output samples per channel (TAC `sample_count`); 0 without audio |
| 36 | 4 | `tac_frames`: TAC frames in the file; `ceil(audio_samples / 1024) + 1` with audio, 0 without |
| 40 | 4 | `index_offset` |
| 44, 48 | 4 + 4 | `sub_offset`, `sub_length` of the subtitle area (both 0 without `subs`) |
| 52 | 8 | reserved, 0 (draft 1.2 had a separate glyph table here) |
| 60 | 4 | CRC32 of bytes 0..59 (always present) |

The TVC stream is the concatenation of the chunks' picture data in chunk order, with the header's size, frame rate and level; it must satisfy that level (TVC section 8).

## 3. Display order and timing (normative; audio is the master clock)
- **Display index.** Pictures are in decoding order. Each picture's display index `disp(n)` (n = chunk number) follows TVC section 1: walk the chunks in order with a counter `out = 0` and no held anchor; a B picture (`picture_type` 2) gets `disp = out`, `out += 1`; an anchor (I or P) first gives the held anchor, if any, `disp = out`, `out += 1`, then becomes the held anchor; after the last chunk the held anchor gets `disp = out`. The display indices are a permutation of 0..frame_count-1.
- **Presentation sample** of display index k: `S(k) = floor(k * 48000 * fps_den / fps_num)` (exact integer arithmetic; it fits 64 bits). Picture k is shown while the audio output position (samples per channel since sample 0) is in `S(k) .. S(k+1) - 1`. Examples: 24/1 gives 2000 samples per picture; 24000/1001 exactly 2002; 25/1 1920; 30000/1001 the repeating pattern 1601, 1602, 1601, 1602, 1602 (8008 per 5 pictures). The per-picture sample count `S(k+1) - S(k)` is fixed by the header and not stored; the index stores `S(disp(n))`, which must fit 32 bits (files up to about 24.8 hours).
- Without audio the same S(k) drives a 48 kHz sample counter.
- **D(n)** = the largest `disp(m)` over chunks m = 0..n.

## 4. Chunk (one per picture, decoding order)
| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `chunk_len`: total length of the chunk including this 16-byte header, without the padding to 4 |
| 4 | 4 | `pic_len`: TVC picture data length, at least 3 |
| 8 | 2 | `n_audio`: number of TAC frames in this chunk (0..65535; 0 without audio) |
| 10 | 2 | chunk flags: bit 0 `key` = (picture_type == 0), bit 1 `drop` = (picture_type == 2); other bits 0 |
| 12 | 4 | CRC32 of chunk bytes 16 .. chunk_len-1 if the header flag `crc` is set, else 0 |
| 16 | pic_len | TVC picture data (TVC section 2; byte 0 is `picture_type`, which must be 0, 1 or 2) |
| ... | | `n_audio` times: `u16 len` (at least 1), then `len` bytes = one TAC frame (TAC section 3) |

`chunk_len` must equal `16 + pic_len + sum over frames of (2 + len)`. The first chunk must be `key`.

### 4.1 Audio interleaving (frame locking)
TAC frames are stored in order over the chunks, every frame in exactly one chunk. `F(n)` = number of TAC frames in chunks 0..n; `F(frame_count - 1) = tac_frames`. Let `need(n) = min(tac_frames, floor(S(D(n) + 1) / 1024) + 2)`. For every chunk n:
- **lower bound:** `F(n) >= need(n)`. (Output sample s depends on TAC frames up to `floor(s / 1024) + 1`, so the audio for the display period of every picture decodable so far is present.)
- **upper bound**, for every chunk except the last: `F(n) <= need(n) + 4` (the audio runs at most about 0.1 s ahead). The last chunk carries whatever audio remains.
The reference muxer stores exactly `F(n) = need(n)` for n < frame_count - 1.

### 4.2 Chunk byte rate (draft 1.2)
For every run of `ceil(fps_num / fps_den)` consecutive chunks, and every shorter run at the start of the file, the sum of their `chunk_len` (video, audio and chunk headers; padding not counted) is at most the TVC level's "bytes in any ceil(fps) consecutive pictures" limit plus **65,536**: 265,536 (level 1), 465,536 (level 2), 665,536 (level 3) bytes. This bounds what the player must read per second of pictures (level 3: 0.67 MB against the measured 850 KB/s card rate); the 64 KiB covers audio up to about 400 kbit/s and the final chunk's audio tail. Muxers trim long audio tails (reference: `-T`) or keep them short.

## 5. Index (one 16-byte entry per chunk, in chunk order)
| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `offset` of chunk n |
| 4 | 4 | `S(disp(n))` |
| 8 | 4 | `first_tac` = F(n-1) (0 for n = 0): the file index of the chunk's first TAC frame (or of the next one stored, if `n_audio` = 0) |
| 12 | 4 | bits 0-1 = the chunk flags; bits 8-31 = `seek_tac` (key chunks with audio only, else 0); bits 2-7 = 0 |

**Reset bit of a TAC frame.** A TAC frame's `reset` (TAC section 3 step 1) is its first bypass bin; with bytes `b[0..len-1]` (missing bytes count as 0) it is 1 exactly when `(b[1] << 24 | b[2] << 16 | b[3] << 8 | b[4]) >= 0x7FFFFFFF`.
**seek_tac** of a key chunk n (with audio) = the largest TAC frame index r with `reset = 1` and `r * 1024 <= S(disp(n))`. Such an r exists because frame 0 has `reset = 1`.

**Seeking (informative procedure, normative data).** To start at sample t: take the last key entry with `S(disp) <= t` (or the first chunk). Decode video from that chunk. Decode audio from TAC frame `seek_tac`, found in the chunk m whose index range `first_tac(m) .. first_tac(m) + n_audio(m) - 1` contains it (m may lie before the key chunk); audio output is valid from sample `seek_tac * 1024` (TAC 7b.5) and is played from `S(disp)` of the key picture. B pictures stored right after the key chunk that need the anchor before it (TVC 7b.11) cannot be decoded after a seek: the player skips them and keeps showing the previous output.

## 6. Subtitles (TST draft 1)
Subtitles are loaded whole at open; they are not interleaved. Shaping, line breaking and layout are the encoder's job (`tools/tst_ref/`, `docs/TST_SUBTITLES_V0.md`); the player only selects, places and draws. Offsets in this section are relative to `sub_offset`. Bitmap events: later draft.

### 6.1 Subtitle area
| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `track_count`, 1..15 |
| 1 | 3 | reserved, 0 |
| 4 | | `track_count` track entries, each `56 + 20 * variant_count` bytes: |
| | 4 | `lang`: ISO 639-2/3 code in lower-case ASCII letters, padded with 0 bytes (e.g. `eng\0`, `por\0`, `und\0`); at least 2 letters |
| | 1 | track flags: bit 0 `default` (at most one track has it), bit 1 `forced_track` (the track holds only lines meant to show while subtitles are off, e.g. translated foreign dialogue); other bits 0 |
| | 1 | `variant_count`, 1..8 |
| | 2 | reserved, 0 |
| | 48 | `palette`: 16 entries of R, G, B bytes (draft 1.4); entry 0 is the ordinary dialogue colour, entries 1..15 are authored (speaker or style colours) |
| | 20 each | variant: `u8 cell_w`, `u8 cell_h`, `u16` reserved 0, `u32 events_offset`, `u32 events_length`, `u32 glyph_offset`, `u32 glyph_length` |

The variants of a track are the same text laid out at different sizes (the player's size setting picks one); their `cell_h` values strictly increase; each variant's `cell_w`/`cell_h` equal those of its glyph table. **Layout:** after the directory, at `align4(directory end)`, come for track 0 variant 0 its event list, then its glyph table, then variant 1's event list and glyph table, and so on through all tracks, each block at `align4` of the previous block's end (zero padding); the offsets in the directory must equal these positions, and `sub_length` ends exactly at the end of the last glyph table.

### 6.2 Event list and glyph table
**Event list** (`events_length` bytes): `u32 count`, then `count` events, each: `u32 start`, `u32 end` (samples, `start < end`), `u16 x`, `u16 y` (top-left of the first glyph cell in the 800 x 720 grid: x < 800, y < 720), `u8 colour` (palette index 0..15; draft 1.3: 0..3), `u8 flags` (bit 0 `forced`: shown even when subtitles are off; bit 1 `sdh`: a description of sounds for the hearing-impaired, which the player may hide; other bits 0), `u16 n`, then `n` glyph references `u16`: a glyph id (below the glyph count and below 0xFFE0), `0xFFE0 + k` (k = 0..15: colour switch to palette entry k), or 0xFFFF (line break); 0xFFF0..0xFFFE are invalid. Events are sorted by `start` (equal starts allowed). The length must equal exactly the sum of its parts.
**Glyph table** (`glyph_length` bytes): `u16 glyph_count` (1..65504), `u8 cell_w`, `u8 cell_h` (both at least 1), then per glyph: `u8 advance` (1..cell_w), then `cell_h` rows of `ceil(cell_w / 4)` bytes, each byte holding 4 two-bit alpha values (0 transparent .. 3 opaque), pixel x of the row in bits `2*(x mod 4)` .. `+1` of byte `x / 4`; padding bits of the last byte of a row are 0. The table length must equal exactly `4 + glyph_count * (1 + cell_h * ceil(cell_w / 4))`.

### 6.3 Drawing (normative)
An event is shown while `start <= p < end`, p = the audio output position in samples (after the player's delay setting, 6.4). Its glyph references are drawn in order from the pen position (x, y): a glyph places its cell with the top-left at the pen and then adds its `advance` to the pen's x; a line break (0xFFFF) sets the pen's x back to the event's x and adds `cell_h` to its y. Each glyph is drawn in the **current colour**, which starts as the event's `colour` and is changed by a colour switch (it stays across line breaks; switches do not move the pen). Where cells of the same or of different active events overlap, the larger alpha wins, and on equal alpha the glyph drawn later (later in the event, or in a later event of the list) wins; the result is the **coverage map** (0..3 per grid pixel) plus, per pixel, the palette index of the glyph that set it. Glyphs may come from any number of fonts: the encoder bakes each (font, character) pair it uses as its own glyph. A space is an ordinary glyph (all alpha 0). An event has one origin, so the encoder centres or aligns text by choosing x, usually one event per line.

### 6.4 Player display parameters (informative; the reference renderer is `tools/tst_ref/tst_render.py`)
These are not stored in the file. The hardware exposes them as registers so any value can be tried; the player's menu may offer a few presets.
- **Selection:** track (or off); size variant; `forced` events shown when subtitles are off; `sdh` events hidden or shown.
- **Timing:** delay in milliseconds, signed (p is shifted before the test of 6.3).
- **Placement:** vertical offset in grid rows, signed (added to every event's y; drawing is clipped to the grid).
- **Text:** the track's palette, except that the player's text colour replaces entry 0 (ordinary dialogue); an "override all colours" switch draws every entry in that colour. Coverage 0..3 blends 0, 1/3, 2/3, 1 of the colour over what is below.
- **Outline:** RGB and width w = 0..4 grid pixels (0 = none): the outline mask is the coverage map dilated by a disc of radius w (grid pixels within distance w), drawn solid under the text.
- **Box:** on/off, RGB, padding 0..16 grid pixels: for each active event, the rectangle around its cells grown by the padding, drawn solid under the outline and text.
- Order, bottom to top: picture, box, outline, text. One grid pixel is 2 x 2 panel pixels, the finest step the 800 x 720 output has. (Shadow and opacity settings were considered and left out by the owner, 2026-10-28.)

## 7. Validity
A demuxer must reject a file that breaks any rule above. Checked by the reference (`tnv3 check`, exit status 3 with a message):
1. magic, version, header CRC, reserved bits and bytes, flag-dependent zero fields, level 1..3, channels, sample rate, `tac_frames` formula, `frame_count >= 1`, frame-rate range;
2. layout (section 1): every offset and length, zero padding, exact file size;
3. chunks: `chunk_len` sum, `pic_len >= 3`, picture type, `key`/`drop` flags, first chunk key, a P picture after at least one anchor and a B picture after at least two (TVC section 2), CRC when `crc` is set (and CRC field 0 when it is not), TAC frame lengths >= 1, frame totals, TAC frame 0 with `reset = 1`;
4. timing: `S(disp(n))` fits 32 bits; interleaving bounds of 4.1;
5. index: every field equal to the value derived from the chunks;
6. TVC level limits that need no picture decoding: coded size, macroblocks per picture and per second, frame rate, bytes per picture, bytes per `ceil(fps)` window (TVC section 8 and 7b.19). Slice counts and vector ranges are the video decoder's job; the chunk byte rate of 4.2;
7. subtitles: the area layout of 6.1 (directory, offsets, padding, exact length), track and variant fields (language letters, one default track at most, increasing `cell_h`, cell sizes equal to the glyph table's), the event list and glyph table rules of 6.2 (sorted events, flags, colour index, glyph references and their reserved codes).
**Player behaviour** on a damaged chunk at run time (informative): repeat the previous picture, mute that chunk's audio frames, resynchronise at the next key chunk.

## 7b. Clarifications (normative; from the independent Python demuxer review of draft 1)
An independent demuxer written only from this text (`tools/tnv3_ref/py/tnv3_demux.py`) accepts and rejects every vector as the C reference does and demuxes all valid ones to identical bytes. Its points of interpretation, pinned down:
1. **Audio length:** with `audio`, `audio_samples = 0` is allowed (then `tac_frames = 1`). The audio may end before or after the video (vectors p07, p09).
2. **Field widths:** width and height are u16; `fps_num` and `fps_den` are u32 fields whose values must be 1..65535 (so they fit the TVC header).
3. **Zero fields:** without `audio`, channels, sample rate, `audio_samples`, `tac_frames` and every `n_audio` are 0; without `subs`, the four subtitle fields are 0. Padding after every chunk (also the last) is zero.
4. **Picture contents:** the container reads only byte 0 (`picture_type`) of a picture. Its qp, flags and slice table belong to the video decoder. A demuxer may check them too; conforming files pass either way.
5. **Rate window:** every run of `ceil(fps_num / fps_den)` consecutive pictures, and every shorter run at the start, as TVC 7b.19. Macroblocks per second and frame rate are compared exactly (`mbs * fps_num <= limit * fps_den`, `fps_num <= 30 * fps_den`).
6. **Index word:** the fourth index field must equal `flags | seek_tac << 8` exactly. `seek_tac` cannot overflow 24 bits because `S` fits 32 bits. For trailing chunks without audio, `first_tac` may equal `tac_frames`.
7. **Subtitles:** `count = 0` and events with `n = 0` are allowed. Every variant has a glyph table with at least one glyph. Event times are not tied to the audio or video duration. The 800 x 720 grid does not depend on the picture size. Sorting is checked between consecutive events.
8. **Rejection reasons** are informative; a file with several faults may be rejected for any of them.
9. **B pictures after a key chunk** that use the anchor before it are valid (TVC 7b.11); only the anchor counts of section 7.3 apply.
10. **Demuxed outputs** (reference tools only): video as a `"TVC1"` file with the header's size, frame rate and level and `frame_count` pictures; audio as a `"TAC0"` file with `tac_frames` and `audio_samples`; the subtitle area as raw bytes; absent streams are not written.

## 8. Informative: overhead
Per chunk 16 B + 2 B per TAC frame + 16 B of index: at 24 fps and 96 kbit/s audio (46.875 TAC frames/s) about 0.86 kB/s, under 1% of a 100 kB/s stream.

## 8b. Informative: the reference film pipeline
`tools/tnv3_ref/encode_film.py SOURCE OUT.tnv3 [--qp 30] [--width 800|native] [--quality]` makes a whole TNV3 file from a film: ffmpeg decodes and scales once (frame rate snapped to 24000/1001, 24, 25, 30000/1001 or 30 when within 0.05%), the frames are cut into 480-frame segments that `tvc_enc` encodes in parallel at the smallest level that fits (each segment starts with an I picture; the encoder enforces the level's per-picture and per-second byte limits, reserving room for the rest of each window, and a segment whose start breaks the window together with the previous segment's tail is re-encoded with that tail, `-P`), `tac enc` makes the 96 kbit/s audio, `tnv3 mux -T -c` interleaves and `tnv3 check` validates. `--quality` decodes every segment with `tvc_dec` and measures VMAF, PSNR-Y and SSIM-Y against the scaled source. `film_sweep.py` runs it per film at several QPs and interpolates the size at VMAF 90.

## 9. Open
The read-ahead buffer size the player needs (decode-ahead of key pictures against the card rate) is to be fixed with the first player measurements; section 4.2 bounds the average, not the burst within a second.

# TVC bitstream v0 (draft 2, 2026-10-27): normative decoding specification

TVC is the video part of TNV3. This document is the **only** normative description of v0: an independent decoder must be implementable from it alone. Reference code: `tools/tvc_ref/` (C).
Scope of v0 draft 2: progressive 8-bit 4:2:0, I and P pictures (anchors) and non-reference B pictures, 16x16 macroblocks, one motion vector per used list per macroblock at half-sample precision, 8x8 integer transform, adaptive binary range coding, in-loop deblocking.
Draft 2 adds B pictures; every draft-1 stream decodes identically under draft 2.
**Draft 3** adds an optional quarter-sample motion mode per picture (flags bit 1, section 2 and 6.3); streams without that bit decode exactly as in draft 2.
**Draft 4** adds an optional 4x4 transform for luma blocks per picture (flags bit 2, sections 4, 5.2 step 6, 5.3, 6.1); streams without that bit decode exactly as in draft 3.
**Draft 5** adds slices (flags bit 3; sections 2, 3, 5, 6.2) and levels with the "TVC1" sequence header (sections 1, 8); "TVC0" files and pictures without bit 3 decode exactly as in draft 4.

Conventions: integers are unbounded unless stated; `>>` is an arithmetic right shift (floor division by a power of two, also for negative values); `/` is used only where stated, as C integer division truncating toward zero; `clip(v, lo, hi)`; `abs`; `min`; `median(a, b, c)` = the middle value. Bit 0 of a byte is its least-significant bit.

## 1. Reference container (.tvc, testing only; the product container is TNV3)
Little endian. `"TVC0"` (4 bytes), `u16 width`, `u16 height`, `u16 fps_num`, `u16 fps_den`, `u32 frame_count`, then for each picture `u32 length` followed by `length` bytes of picture data.
**Draft 5:** magic `"TVC1"` has the same fields followed by `u8 level` (1, 2 or 3; section 8) and 3 reserved bytes (0), i.e. a 20-byte header; the stream must then satisfy that level. A `"TVC0"` file has no level (unconstrained).
Pictures are stored in **decoding order**. **Anchors** are I and P pictures; B pictures are never used for prediction.
**Reference anchors:** the decoder keeps the *latest* anchor and the *previous* anchor (the one decoded before the latest). After an anchor is decoded it becomes the latest and the former latest becomes the previous.
**Output (display) order:** a B picture is output as soon as it is decoded. An anchor is held: when the next anchor has been decoded, the held anchor is output first and the new anchor is held; at the end of the stream the held anchor is output. (Example: decoding order I0 P3 B1 B2 P6 B4 B5 gives display order I0 B1 B2 P3 B4 B5 P6.)

## 2. Picture data
| Offset | Field |
|---|---|
| 0 | `picture_type`: 0 = I, 1 = P, 2 = B (other values invalid) |
| 1 | `picture_qp`: 0..51 |
| 2 | `flags`: bit 0 = `aq` (macroblock QP deltas present); bit 1 = `qpel` (draft 3: motion vectors are in quarter luma samples; otherwise half samples); bit 2 = `t4ok` (draft 4: luma blocks may use four 4x4 transforms); bit 3 = `sliced` (draft 5); other bits 0 |
| 3.. | if `sliced = 0`: range-coded data of the single slice (section 3), to the end of the picture |
| 3 | if `sliced = 1`: `rows` = macroblock rows per slice (1..255); the picture has `ns = ceil(mbh / rows)` slices, slice k covering rows k*rows .. min((k+1)*rows, mbh) - 1 |
| 4.. | if `sliced = 1`: `ns - 1` values `u32` (little endian) = the byte length of slices 0 .. ns-2; then the data of slice 0, 1, ..., ns-1 concatenated (the last slice runs to the end of the picture) |

A P picture requires a latest anchor; a B picture requires both a latest and a previous anchor.
**Lists.** P pictures: list 0 = the latest anchor (decoded before this P). B pictures: list 0 = the previous anchor, list 1 = the latest anchor. (For a P picture "latest" means the anchor before it, i.e. before the P itself is decoded.)

**Coded size.** `W = (width + 15) & ~15`, `H = (height + 15) & ~15`; `mbw = W / 16`, `mbh = H / 16`. Luma planes are W x H, chroma planes (Cb, Cr) are W/2 x H/2. Decoding produces full coded-size planes; output is the top-left width x height luma and ((width+1)/2) x ((height+1)/2) chroma.

## 3. Range decoder
State: 32-bit unsigned `range`, `code`. Input bytes are read in order; reading past the end of the picture data yields 0.
**Init:** `range = 0xFFFFFFFF`; `code = 0`; repeat 5 times: `code = ((code << 8) | next_byte) mod 2^32`.
**Normalise:** while `range < 2^24`: `range = (range << 8) mod 2^32`, `code = ((code << 8) | next_byte) mod 2^32`.
**Context bin** with probability `p` (an integer 1..32767, the probability of a 0 bit scaled by 32768):
`bound = (range >> 15) * p`; if `code < bound`: `bit = 0`, `range = bound`; else `bit = 1`, `code -= bound`, `range -= bound`. Then update `p`: if `bit == 0`: `p += (32768 - p) >> 5`, else `p -= p >> 5`. Then normalise.
**Bypass bin:** `range = range >> 1`; if `code >= range`: `bit = 1`, `code -= range`, else `bit = 0`. Then normalise.
**Initial probability** of every context: 16384. All contexts are reset at the start of every picture.
**Slices (draft 5):** every slice has its own range decoder (initialised on that slice's bytes; reading past the end of the slice yields 0) and starts with all contexts reset.

**EG(ctx[0..n-1], k)**, adaptive Exp-Golomb of order k: `i = 0`, `base = 0`; while a context bin with `ctx[min(i, n-1)]` decodes 1: `base += 1 << k`, `k += 1`, `i += 1`. Then read `k` bypass bins, most significant first, as an unsigned number `s`. Result `base + s`. (A conforming stream never needs k > 24.)
**SEG(zctx, mctx[0..n-1])**, signed value: if a context bin with `zctx` decodes 0, the value is 0. Otherwise `m = 1 + EG(mctx, 0)`, then a bypass bin `s`; value = `-m` if `s == 1`, else `m`.

## 4. Contexts (all probabilities, reset to 16384 per picture)
| Name | Count | Used for |
|---|---|---|
| skip[3] | 3 | macroblock skip flag |
| intra[3] | 3 | intra flag in P and B pictures |
| bdir[2] | 2 | prediction lists of an INTER macroblock in B pictures |
| mpm[1], rem[2] | 3 | luma intra modes |
| cmode[3] | 3 | chroma intra mode |
| cbp[3][3] | 9 | coded-block flags (index 0 luma, 1 Cb, 2 Cr) |
| dqz[2], dqm[3] | 5 | macroblock QP delta |
| mvz[2][3], mvm[2][6] | 18 | motion vector difference (index 0 = x, 1 = y) |
| last[3][64] | 192 | last significant position (class 0 luma 8x8, 1 chroma 8x8, 2 luma 4x4; class 2 uses entries 1..15) |
| sig[3][13][3] | 117 | significance |
| gt1[3][3][3] | 27 | level greater than one |
| lrem[3][6] | 18 | level remainder |
| t4[3], cbf4[4] | 7 | draft 4: transform-size flag, 4x4 sub-block coded flags |

## 5. Macroblock layer
Macroblocks are decoded in raster order (`y` from 0 to mbh-1, `x` from 0 to mbw-1). Per macroblock the decoder keeps: `type` (INTRA, INTER, SKIP), `uses` (bit 0 = list 0, bit 1 = list 1), `mv[L] = (mvx, mvy)` and `mvd[L]` for L = 0, 1 (zero for an unused list), `coded` (6 bits), `imode[4]`, `cmode`, `qp`. INTRA: `uses = 0`. P pictures: SKIP and INTER use list 0 only (`uses = 1`). A neighbour is **unavailable** if it lies outside the picture **or in another slice** (draft 5; i.e. a row before the current slice's first row); left (x-1, y) and above (x, y-1) are always already decoded when available.

Per slice state (per picture when there are no slices): `prev_qp = picture_qp`, `prev_dq = 0` at the start of each slice.

### 5.1 Motion vector prediction (P and B pictures, computed before parsing each macroblock, for each list L = 0, 1)
Let A = left, B = above, C = above-right (x+1, y-1); if C is unavailable use D = above-left (x-1, y-1) instead. A neighbour's vector for list L is its `mv[L]` if bit L of its `uses` is set, otherwise (0, 0) (so INTRA neighbours give (0,0); SKIP macroblocks carry their predicted vectors).
If B is unavailable: `pred[L]` = A's vector for L if A is available, else (0, 0). Otherwise `pred[L] = (median(Ax, Bx, Cx), median(Ay, By, Cy))` of the three neighbours' list-L vectors, using (0, 0) for an unavailable A or C.

### 5.2 Syntax, in order
1. **P and B pictures: skip flag.** context `skip[n]`, n = number of {left, above} that are available and SKIP. If 1: `type = SKIP`, `uses = 1` in P pictures and `3` (both lists) in B pictures, `mv[L] = pred[L]` for each used list, all `mvd = (0,0)`, `coded = 0`, `qp = prev_qp`; the macroblock ends here.
   Else **intra flag**: context `intra[n]`, n = number of {left, above} available and INTRA. 1 = INTRA, 0 = INTER.
   I pictures: `type = INTRA`, no flags.
2. **QP delta** (only if `aq`): `d = SEG(dqz[prev_dq != 0 ? 1 : 0], dqm[0..2])`; `qp = clip(prev_qp + d, 0, 51)`; `prev_qp = qp`; `prev_dq = d`. If not `aq`: `qp = picture_qp`.
3. **INTRA:** `mv = (0, 0)`, `mvd = (0, 0)`.
   - Four luma modes, blocks b = 0..3 (b = 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right). Left mode `ml` = `imode[b-1]` of this macroblock if b is 1 or 3, else `imode[b+1]` of the left macroblock. Above mode `mt` = `imode[b-2]` of this macroblock if b is 2 or 3, else `imode[b+2]` of the above macroblock. A neighbour that is unavailable or not INTRA gives mode 0. `mpm = min(ml, mt)`.
     Bin with `mpm[0]`: 1 means `imode[b] = mpm`. Otherwise let `others` = the three modes other than mpm in increasing order; bin with `rem[0]`: 0 means index 0; else bin with `rem[1]`: index = 1 + that bin. `imode[b] = others[index]`.
   - Chroma mode: bin with `cmode[0]`: 0 means cmode = 0; else bin with `cmode[1]`: 0 means cmode = 1; else cmode = 2 + bin with `cmode[2]`.
   Modes: 0 = DC, 1 = vertical, 2 = horizontal, 3 = planar.
4. **INTER:** in P pictures `uses = 1`. In B pictures: bin with `bdir[0]`: 0 means `uses = 3` (both lists); 1 means a single list, then bin with `bdir[1]`: 0 gives `uses = 1` (list 0), 1 gives `uses = 2` (list 1).
   Then for L = 0, 1, if bit L of `uses` is set: for component k = x then y: `s` = sum of `abs(mvd[L][k])` of the left and above macroblocks that are available, INTER, and use list L (others count 0); class = 0 if s < 2, 1 if s < 16, else 2; `mvd[L][k] = SEG(mvz[k][class], mvm[k][0..5])`. (The contexts are indexed by the component k, shared by both lists.) `mv[L] = pred[L] + mvd[L]`.
5. **Coded-block flags** (all types except SKIP): luma blocks b = 0..3 in order: left flag `lc` = bit (b-1) of this macroblock's `coded` if b is 1 or 3 (already decoded), else bit (b+1) of the left macroblock's `coded`; above flag `tc` = bit (b-2) of this macroblock if b is 2 or 3, else bit (b+2) of the above macroblock. Unavailable gives 0. Bin with `cbp[0][lc + tc]` gives bit b. Then Cb (bit 4) and Cr (bit 5): `lc`/`tc` = the same bit of the left/above macroblock; bins with `cbp[1][..]` and `cbp[2][..]`.
6. **Coefficient blocks** for each set bit of `coded` in the order bit 0..5 (luma 0-3, Cb, Cr): section 5.3 with class `cc` = 0 for luma, 1 for chroma.
   **Draft 4, when `t4ok = 1`:** for a coded luma block b, first a **transform-size flag** with context `t4[lt + tt]`, where `lt` / `tt` are the flags of the left / above luma block found as for the coded-block flags in step 5 (own block b-1 / b-2 when inside the macroblock, else the neighbouring macroblock's block b+1 / b+2; 0 if unavailable, not coded, or SKIP). Flag 0: one 8x8 block as before. Flag 1: for sub-blocks k = 0..3 (0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right, 4x4 each) a sub-block coded flag with context `cbf4[k]`, each immediately followed, if 1, by a 4x4 coefficient block (section 5.3, N = 16, class 2). The flags of a macroblock that are not decoded (uncoded blocks, SKIP, `t4ok = 0`) are 0. Chroma never uses 4x4.

### 5.3 Coefficient block
Levels `lev[0..63]` in scan order start at 0.
1. **last**: `node = 1`; repeat 6 times: `bit` = bin with `last[cc][node]`; `node = 2*node + bit`. `last = node - 64` (0..63).
2. For `i = last` down to 0:
   - if `i < last`: `n = (lev[i+1] != 0) + (i+2 <= last and lev[i+2] != 0)`; significance bin with `sig[cc][PCLASS[i]][n]`; if 0, continue with the next i. (At `i == last` the coefficient is significant without a bin.)
   - `g` = bin with `gt1[cc][gc][min(ngt1, 2)]`, `gc = 0` if i < 6, 1 if i < 20, else 2; `ngt1` counts the levels > 1 already decoded in this block (starts at 0).
   - magnitude `a = 1` if g == 0, else `a = 2 + EG(lrem[cc][0..5], 0)` and `ngt1 += 1`.
   - sign: bypass bin, 1 = negative. `lev[i] = ±a`.
`PCLASS[0..63]` = 0,1,2,3,4,5,6,6,7,7,8,8,8,8, then 9 (positions 14-19), 10 (20-29), 11 (30-41), 12 (42-63).
**4x4 blocks (draft 4, N = 16, class 2):** the same procedure with these changes: `last` uses 4 bins (`node` from 1, `last = node - 16`, contexts `last[2][1..15]`); the significance class is `PCLASS4[i]` = 0,1,2,3,4,5,6,6,7,7,7,7,8,8,8,8; `gc = 0` if i < 3, 1 if i < 8, else 2. Scan order: the same rule as for 8x8 with rows and columns 0..3 (index 0 = (0,0), 1 = (0,1), 2 = (1,0), 3 = (2,0), 4 = (1,1), 5 = (0,2), ...).

**Scan order** (scan index -> (row, column) inside the 8x8 block, row = vertical frequency): for s = 0..14, positions with row + column = s; for odd s in increasing row order, for even s in decreasing row order, rows limited to 0..7 and columns to 0..7. (Index 0 = (0,0), 1 = (0,1), 2 = (1,0), 3 = (2,0), 4 = (1,1), 5 = (0,2), ...)

## 6. Reconstruction
### 6.1 Dequantisation and inverse transform
`QS[0..51]` = 10, 11, 13, 14, 16, 18, 20, 23, 25, 29, 32, 36, 40, 45, 51, 57, 64, 72, 81, 91, 102, 114, 128, 144, 161, 181, 203, 228, 256, 287, 323, 362, 406, 456, 512, 575, 645, 724, 813, 912, 1024, 1149, 1290, 1448, 1625, 1825, 2048, 2299, 2580, 2896, 3251, 3649.
Coefficient at the scan position's (row u, column v): `c = sign(l) * min(32767, (abs(l) * QS[qp] + 8) >> 4)` (0 for l = 0), `qp` = the macroblock's qp.
Matrix `T[k][n]` (k = row):
```
 64  64  64  64  64  64  64  64
 89  75  50  18 -18 -50 -75 -89
 84  35 -35 -84 -84 -35  35  84
 75 -18 -89 -50  50  89  18 -75
 64 -64 -64  64  64 -64 -64  64
 50 -89  18  75 -75 -18  89 -50
 35 -84  84 -35 -35  84 -84  35
 18 -50  75 -89  89 -75  50 -18
```
Stage 1, for every y, v in 0..7: `e[y][v] = clip((sum over u of T[u][y] * C[u][v] + 64) >> 7, -32768, 32767)`.
Stage 2, for every y, x: `r[y][x] = (sum over v of e[y][v] * T[v][x] + 128) >> 8`.
A block whose `coded` bit is 0 has `r = 0`. Sample = `clip(pred + r, 0, 255)`.
**4x4 (draft 4).** For a luma block with transform-size flag 1, each sub-block k covers rows 4*(k>>1)..+3 and columns 4*(k&1)..+3 of the 8x8 residual; an uncoded sub-block has r = 0. Dequantisation as above (same qp); matrix `T4` = rows `(64, 64, 64, 64)`, `(84, 35, -35, -84)`, `(64, -64, -64, 64)`, `(35, -84, 84, -35)`. Stage 1: `e[y][v] = clip((sum over u of T4[u][y] * C[u][v] + 64) >> 7, -32768, 32767)`; stage 2: `r[y][x] = (sum over v of e[y][v] * T4[v][x] + 64) >> 7` (y, x, u, v in 0..3). Prediction (intra or inter) is still formed for the whole 8x8 block before the residual is added.

### 6.2 Intra prediction (8x8, from the current picture before deblocking)
Block at (bx, by) in a plane of width w. Above row `Tn[i]` = sample (bx+i, by-1), available if by > top, where top = the first sample row of the current slice in this plane (16 x its first macroblock row for luma, 8 x for chroma; 0 without slices); left column `Ln[i]` = sample (bx-1, by+i), available if bx > 0. Within a picture, samples of earlier macroblocks and earlier blocks of the current macroblock are final reconstructions (deblocking happens later). P pictures predict intra blocks from neighbouring INTER/SKIP reconstructions too.
- **DC (0):** both available: `(sum T + sum L + 8) >> 4`; only one: `(sum of that side + 4) >> 3`; none: 128.
- For modes 1-3, substitute first: if above is unavailable, every `T[i] = L[0]` if left is available else 128; then if left is unavailable, every `L[i] = T[0]` if above is available else 128.
- **Vertical (1):** `P[y][x] = T[x]`. **Horizontal (2):** `P[y][x] = L[y]`.
- **Planar (3):** `P[y][x] = ((7-x)*L[y] + (x+1)*T[7] + (7-y)*T[x] + (y+1)*L[7] + 8) >> 4`.
Luma: blocks b = 0..3 at (16x + 8(b&1), 16y + 8(b>>1)), each predicted and reconstructed before the next. Chroma: one block per plane at (8x, 8y) with mode `cmode`, Cb then Cr.

### 6.3 Inter prediction (P and B pictures)
Reference of list L = the anchor named in section 2 (after its deblocking), at coded size. For a block at (bx, by), size bw x bh, vector (mvx, mvy) in half-sample units: `ix = mvx >> 1`, `iy = mvy >> 1`, `fx = mvx & 1`, `fy = mvy & 1`. For each sample, `x0 = clip(bx+x+ix, 0, w-1)`, `x1 = clip(bx+x+ix+1, 0, w-1)`, `y0`, `y1` likewise with h; `a = R(x0,y0)`, `b = R(x1,y0)`, `c = R(x0,y1)`, `d = R(x1,y1)`.
Value: `a` if fx = fy = 0; `(a + b + 1) >> 1` if only fx; `(a + c + 1) >> 1` if only fy; `(a + b + c + d + 2) >> 2` if both.
For each used list L: luma one 16x16 block with `mv[L]` from list L's reference; chroma 8x8 blocks with `(mvx / 2, mvy / 2)` of `mv[L]` (C division, truncating toward zero), same formula in the chroma plane.
The paragraphs above apply when `qpel = 0`. **When `qpel = 1`** (draft 3) vectors are in quarter luma samples and the single-list prediction is instead:
- **Luma** (16x16 at (bx, by)): `ix = mvx >> 2`, `iy = mvy >> 2`, `fx = mvx & 3`, `fy = mvy & 3`. `R(X, Y)` = reference sample at (clip(X, 0, W-1), clip(Y, 0, H-1)). Filters `F[0] = (0, 64, 0, 0)`, `F[1] = (-4, 56, 14, -2)`, `F[2] = (-4, 36, 36, -4)`, `F[3] = (-2, 14, 56, -4)`, applied to the taps at offsets -1, 0, +1, +2. For each sample, `X = bx + x + ix`, `Y = by + y + iy`:
  - fx = fy = 0: `R(X, Y)`;
  - fy = 0: `(sum over k of F[fx][k] * R(X+k-1, Y) + 32) >> 6`;
  - fx = 0: `(sum over k of F[fy][k] * R(X, Y+k-1) + 32) >> 6`;
  - both: `t(j) = sum over k of F[fx][k] * R(X+k-1, Y+j-1)` for j = 0..3 (no rounding), value `(sum over j of F[fy][j] * t(j) + 2048) >> 12`;
  - the result is clipped to 0..255.
- **Chroma** (8x8 at (bx, by) in each chroma plane, using the **same** `mv[L]`, which is in 1/8 chroma samples): `ix = mvx >> 3`, `iy = mvy >> 3`, `fx = mvx & 7`, `fy = mvy & 7`; value `((8-fx)*(8-fy)*R(X, Y) + fx*(8-fy)*R(X+1, Y) + (8-fx)*fy*R(X, Y+1) + fx*fy*R(X+1, Y+1) + 32) >> 6` with `X = bx + x + ix`, `Y = by + y + iy` and R clamped to the chroma plane (W/2 x H/2). No truncating division is used in this mode.
If both lists are used, each sample is `(p0 + p1 + 1) >> 1` of the two single-list predictions (each already rounded as above); otherwise the single prediction.
SKIP: prediction only (no residual). INTER: prediction plus each coded block's residual (luma blocks as in 6.2 positions, chroma at (8x, 8y)).

## 7. Deblocking (after all macroblocks of the picture; the result is output and becomes the reference)
Filter of one line across an edge, samples p1, p0 | q0, q1, strength `bS` (1 or 2) and `qp`: `qs = QS[qp]`, `alpha = ((5*qs) >> 5) + 1`, `beta = (qs >> 4) + 1`, `tc = max(1, (bS*qs) >> 6)`. If `abs(p0-q0) < alpha` and `abs(p1-p0) < beta` and `abs(q1-q0) < beta`: `d = clip(((q0-p0)*4 + (p1-q1) + 4) >> 3, -tc, tc)`, `p0 = clip(p0+d, 0, 255)`, `q0 = clip(q0-d, 0, 255)`. All four inputs are read before either output is written.
**Luma.** First every vertical edge x = 8, 16, ..., W-8 for every row y (p1 = (x-2, y) ... q1 = (x+1, y)); then every horizontal edge y = 8, ..., H-8 for every column x, using the vertically filtered samples. For an edge between 8x8 blocks P and Q (in 8x8 units; the block index inside its macroblock is `2*(by&1) + (bx&1)`): `bS = 2` if either macroblock is INTRA; else 1 if either block's `coded` bit is set; else 1 if P and Q are in different macroblocks and their **motion differs**; else 0 (no filtering). Motion differs if the two macroblocks' `uses` differ, or if for some list L used by both `abs(mv[L] x difference) >= thr` or `abs(mv[L] y difference) >= thr`, with `thr = 2` when `qpel = 0` and `thr = 4` when `qpel = 1` (one luma sample in both cases). `qp = (qp_P + qp_Q + 1) >> 1` of the two macroblocks.
**Chroma** (each of Cb, Cr): edges at multiples of 8 chroma samples (macroblock boundaries), vertical edges then horizontal edges as for luma. `bS = 2` if either macroblock is INTRA; else 1 if either macroblock has `coded != 0`; else 1 if their motion differs (as for luma); else 0. Same `qp` rule.

## 7b. Clarifications (normative; from the independent Python decoder reviews of drafts 1-4)
An independent decoder written only from this text (`tools/tvc_ref/py/tvc_dec.py`) decodes all conformance vectors identically to the C reference. Its author listed the places a reader could go wrong; they are pinned down here (normative):
1. **QP state.** `prev_qp` and `prev_dq` change only in step 2 of 5.2 (QP delta), i.e. only for non-SKIP macroblocks of `aq` pictures; `d = 0` sets `prev_dq = 0`. A SKIP macroblock stores `qp = prev_qp` as it is at that moment, and **that stored qp is what deblocking uses** for its edges.
2. **Vector prediction when C and D are both unavailable:** use (0, 0) for C.
3. **mvd context:** the x class uses only the neighbours' `abs(mvd[L] x)`, the y class only `abs(mvd[L] y)`, of the same list L.

Draft 2 additions (from the second independent review):
10. **B pictures** are decoded into their own picture buffer and are never a reference; deblocking applies to them as to anchors, with their own qp, coded flags, lists and vectors.
11. **An I picture is an ordinary anchor for the lists:** it does not reset them. B pictures decoded after an I picture predict from the anchor before the I (list 0) and the I (list 1), even across a GOP boundary. A B picture without a previous anchor is invalid.
12. **pred[L] is computed per list** from the neighbours' list-L vectors only, for both lists, whether or not the current macroblock uses them; it is consumed only for used lists. A SKIP macroblock in a B picture uses both lists and is bi-predicted (averaged) like an INTER macroblock with `uses = 3`.
13. **Vector order** for `uses = 3`: list 0 x, list 0 y, list 1 x, list 1 y. Residuals are added to the averaged prediction.
4. **No vector range limit** in v0 draft 1: `mv = pred + mvd` without clipping; samples outside the picture come from the clamping in 6.3. (Level limits on vector range will be added with the levels.)
5. **Coefficient placement:** scan index i gives (row u, column v); the dequantised value is placed at `C[u][v]` and stage 1 sums over the row index u: `sum_u T[u][y] * C[u][v]`. Do not transpose.
6. **Intra substitution and planar:** the substitutions are applied in the stated order (above first, then left, the left one reading the *original* above row when it is available); planar uses `T[7]` and `L[7]` after substitution.
7. **Chroma vector:** `cmv = mv / 2` (truncating), and the chroma `ix, iy, fx, fy` are derived from `cmv` with the same `>> 1` / `& 1` rules.
8. **Reference and filtering size:** prediction clamps to the coded size (W x H, or W/2 x H/2), deblocking runs over the coded size, and only the output is cropped.
9. **Container header** is 16 bytes; picture lengths have no padding.

14. **qpel (draft 3)** is a per-picture flag; it changes only the vector units, the prediction filters of 6.3 and the deblocking threshold. Syntax, contexts and vector prediction are unchanged (vectors are simply interpreted in the picture's units). A conforming encoder uses the same value for all pictures of a stream.
15. **qpel arithmetic:** in the two-dimensional luma case the horizontal filter is applied first (per row, unrounded, unclipped) and the vertical filter second; clipping happens only on the final value. Negative vectors use floor shifts (`>> 2`, `>> 3`) and non-negative remainders (`& 3`, `& 7`), as everywhere in this document. Bi-prediction averages the clipped single-list predictions in both modes.

17. **4x4 syntax order (draft 4):** the transform-size flag exists for every coded luma block of INTRA and INTER macroblocks when `t4ok = 1`; for a 4x4 block the order is cbf4[0], its coefficients (if 1), cbf4[1], its coefficients, and so on, before the next luma block; chroma blocks follow all luma blocks. The rest of the 4x4 coefficient procedure (neighbour count for significance, `ngt1`, remainder coding) is exactly the 8x8 one with class 2 contexts, and dequantisation uses the same formula with no extra scaling. A 4x4 block may have all four sub-block flags 0 (its residual is then 0, and it still counts as coded for deblocking).
16. **4x4 and deblocking (draft 4):** deblocking is unchanged: only 8x8 block edges are filtered (edges inside a 4x4-coded block are not), and a block counts as coded for the boundary strength when its 8x8 `coded` bit is 1, whatever its sub-block flags.

19. **Level checks (draft 5), precisely:** all limits are inclusive ("at most"); sizes use the coded W and H (each compared with its own limit) and the coded macroblock count; the frame rate is fps_num / fps_den, compared without rounding; "bytes per picture" is the picture's `length` field (header, slice table and slice data, without the 4-byte length prefix); the rate window is every run of `ceil(fps)` consecutive pictures in decoding order, sliding by one, and at the start of the stream the pictures so far (a shorter run) are checked too; the slice minimum applies to every picture; the vector limit applies to every used list of every INTER and SKIP macroblock, in the picture's units (x 2 or x 4). A level byte other than 1..3 makes the file invalid. On a violation the decoder stops and reports failure (the C reference exits with status 3); any output written so far must not be presented as valid. Negative conformance vectors: n01 (size), n02 (rate window), n03 (slice minimum).
18. **Slices and deblocking / inter prediction (draft 5):** slices cut only the parsing and intra/vector-prediction dependencies of section 5 and the intra sample prediction of 6.2. Deblocking (section 7) runs over the whole picture after all slices, across slice boundaries, exactly as without slices; motion compensation reads the reference pictures anywhere.

## 8. Levels (draft 5)
A `"TVC1"` stream declares a level; a decoder built for level N must decode every stream of level 1..N and must **reject** a stream that violates its declared level (the C reference exits with status 3, "level violation"). Coded size = W x H (multiples of 16), macroblocks = mbw x mbh, fps = fps_num / fps_den.

| Limit | Level 1 | Level 2 | Level 3 |
|---|---|---|---|
| coded width x height, at most | 400 x 240 | 800 x 464 | 800 x 720 |
| macroblocks per picture | 375 | 1,450 | 2,250 |
| frame rate | 30 | 30 | 30 |
| macroblocks per second (macroblocks x fps) | 11,250 | 43,500 | 67,500 |
| bytes per picture (the `length` field) | 96,000 | 256,000 | 384,000 |
| bytes in any `ceil(fps)` consecutive pictures (decoding order) | 200,000 | 400,000 | 600,000 |
| slices per picture, at least | 1 | 1 | 2 |
| vector component magnitude, luma samples (x / y) | 256 / 128 | 256 / 128 | 256 / 128 |

Notes (informative): level 2 covers 16:9 film at 800x450 and 2.39:1 at 800x334; level 3 covers the full 800x720 output window and 4:3 at 800x600; the rate limits keep video well under the measured 850 KB/s card rate with room for audio and subtitles; level 3 requires two slices so a two-lane decoder can parse both halves at once. Measured cost of slices (draft-5 encoder, QP 30): 2 slices at 800x600 about +1%, 3 slices at 800x334 about +5% at equal quality, one slice per macroblock row about +26%: use as few as the level requires.

## 9. Known limits of draft 5
No error resilience (a corrupt picture is undefined behaviour; slices limit a parsing error to one slice but concealment is not specified); contexts are not carried between pictures (measured at about -1% and rejected to keep pictures independent); the decode buffer and output timing belong to the TNV3 container, not to this document.

# TAC bitstream v0 (draft 1.2, 2026-10-28): the TNV3 audio codec
Draft 1.2 (same day): conformance tolerance tightened to +-1 / 0.3 RMS, value limit of +-2^26 in section 6, integer decoder model (section 10), vectors t07 (stress) and t08 (crafted limits).

TAC is the audio part of TNV3: an MDCT transform codec for 48 kHz, 16-bit, mono or stereo, aimed at 96-128 kbit/s. This document describes the decoder; the reference is `tools/tac_ref/tac.c` (`tac dec`). The encoder (`tac enc`) is informative. Draft 1 has not been reviewed by an independent implementation yet (the TVC process: next step).
Ideas used, none copied: the MDCT with sine windows and time-domain aliasing cancellation, block switching with transition windows, scale-factor bands on a Bark-like grid, a 3/4-power quantiser law, mid/side stereo and noise filling are textbook transform-coding techniques; the tables, syntax, contexts and entropy coder here are TAC's own (the range coder is the one of TVC, `docs/TVC_BITSTREAM_V0.md` section 3).

## 1. Reference container (.tac, testing only; the product carries TAC frames in TNV3 packets)
Little endian: `"TAC0"`, `u32 sample_rate` (48000), `u8 channels` (1 or 2), 3 reserved bytes, `u32 frame_count`, `u32 sample_count`, then per frame `u16 length` and `length` bytes of frame data (one range-coded stream).

## 2. Signal framing
Frame t (t = 0 .. frame_count-1) carries 1024 spectral values per channel. Its inverse transform produces 2048 samples that are added (overlap-add) at output positions `(t-1)*1024 .. (t+1)*1024 - 1`; positions below 0 are discarded. After all frames, output samples `0 .. sample_count-1` are rounded (`lround`) and clipped to -32768..32767. (The encoder therefore sees N zero samples before the signal and `frame_count = ceil(sample_count/1024) + 1`.)

## 3. Frame syntax (one range-decoder per frame, section 3 of the TVC spec: same `rc_bit`, `rc_bypass`, `EG`)
In order (B(n) = n bypass bins, most significant first):
1. `reset` B(1): if 1, all contexts (section 4) are set to 16384 before anything else is read; otherwise they keep their values from the previous frame. Frame 0 must have `reset = 1`; a decoder may start at any frame with `reset = 1` (the reference encoder sets it every 16 frames).
2. `wtype` B(2): 0 LONG, 1 START, 2 SHORT, 3 STOP. Allowed sequences: LONG -> LONG|START, START -> SHORT, SHORT -> SHORT|STOP, STOP -> LONG|START.
3. `msmode` B(2), stereo only (mono: 0): 0 = left/right, 1 = mid/side in every band, 2 = per-band flags.
4. `maxband` B(6): number of coded bands (at most 42 for LONG/START/STOP, 13 for SHORT).
5. `nfill` B(1): noise filling on/off.
6. if `msmode = 2`: for b = 0 .. maxband-1 a bin with context `ms[prev]` (prev = previous band's flag, 0 for b = 0) = `ms[b]`.
7. For each channel (0, then 1):
   - `sf[0]` B(8); for b = 1 .. maxband-1: delta d: bin with `sfz[pd != 0]` (pd = previous delta, 0 at b = 1); if 1, `m = 1 + EG(sfm[0..5], 0)` and a bypass sign bin (1 = negative); `sf[b] = sf[b-1] + d` (must stay in 0..255).
   - For b = 0 .. maxband-1: **band-zero flag** with context `bz[class][prevz]` (prevz = previous band's zero flag in this channel, 0 for b = 0). If 1, the band's levels are 0. Otherwise its lines, in coding order (section 5), each: with `nc = min(|q1|, 2) + min(|q2|, 2)` (q1, q2 = magnitudes of the two previously coded lines of this channel in this frame, over all non-zero bands; 0 at the start): bin `m0[class][nc]` (0 -> magnitude 0); else bin `m1[class][nc]` (0 -> 1); else bin `m2[class][min(nc, 2)]` (0 -> 2); else `3 + EG(esc[0..5], 0)`. A non-zero magnitude is followed by a bypass sign bin. Magnitudes above 8191 are invalid.
   `class` of band b: LONG-type frames 0 if b < 12, 1 if b < 28, else 2; SHORT frames 0 if b < 4, 1 if b < 9, else 2.

## 4. Contexts (probabilities as in TVC, initial 16384)
`sfz[2]`, `sfm[6]`, `ms[2]`, `bz[3][2]`, `m0[3][5]`, `m1[3][5]`, `m2[3][3]`, `esc[6]`. They are shared by both channels and kept across frames until a `reset`.

## 5. Bands and coding order
Long frames (LONG, START, STOP): 1024 lines, band b = lines `BL[b] .. BL[b+1]-1` with
`BL` = 0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 57, 62, 67, 73, 79, 86, 93, 101, 110, 120, 131, 143, 157, 172, 188, 206, 225, 246, 268, 292, 319, 349, 384, 426, 478, 546, 642, 793, 853 (42 bands; line k is centred at (k + 0.5) x 23.4375 Hz).
SHORT frames: 8 windows of 128 lines; value index `w*128 + k`; band b = lines `BS[b] .. BS[b+1]-1` of **every** window, `BS` = 0, 4, 8, 12, 16, 20, 24, 29, 35, 42, 51, 65, 92, 106 (13 bands); coding order within a band: window 0's lines, then window 1's, ..., window 7's. One scale factor per band covers all 8 windows.
Lines at or above the end of band `maxband-1` are 0.

## 6. Reconstruction
- **Dequantisation** of line value q in band b: `x = sign(q) * |q|^(4/3) * 2^((sf[b] - 100) / 4)`.
- **Noise filling:** if `nfill = 1`, a band b >= 8 (LONG-type) or b >= 3 (SHORT) whose levels are all 0 is filled instead with `x = 2^((sf[b] - 100) / 4) * u`, where u is drawn per line in coding order from a 32-bit generator `s = s * 1664525 + 1013904223 (mod 2^32)`, `u = ((s >> 8) & 0xFFFF) / 32768 - 1`, with `s` set to `t*2 + channel + 1` at the start of each channel of frame t (t = frame index from the start of the file).
- **Limit (draft 1.2):** every value from the dequantisation or the noise filling, and every value produced by the mid/side step below, is clipped to -2^26 .. 2^26 (no 16-bit signal produces MDCT values above about 2^25.4, so real streams never reach it; it bounds the hardware word width and makes extreme valid streams defined; vector t08).
- **Stereo:** for every band b < maxband that is mid/side (`msmode = 1`, or `msmode = 2` and `ms[b] = 1`): `L = M + S`, `R = M - S` per line (channel 0 carries M, channel 1 S); other bands are left/right.
- **Inverse transform** (per channel):
  - LONG-type: `y[n] = (2/1024) * w(n) * sum over k of X[k] * cos(pi/1024 * (n + 0.5 + 512) * (k + 0.5))`, n = 0..2047, with window `w` = LONG: `sin(pi (n+0.5)/2048)`; START: that for n < 1024, 1 for 1024 <= n < 1472, `ws(128 + n - 1472)` for 1472 <= n < 1600, 0 above; STOP: 0 for n < 448, `ws(n - 448)` for 448 <= n < 576, 1 for 576 <= n < 1024, `sin(pi (n+0.5)/2048)` for n >= 1024; where `ws(m) = sin(pi (m+0.5)/256)`.
  - SHORT: for w = 0..7, `y[448 + 128w + n] += (2/128) * ws(n) * sum over k of X[w*128+k] * cos(pi/128 * (n + 0.5 + 64) * (k + 0.5))`, n = 0..255 (other positions 0).
  - The 2048 values are added into the output as in section 2.

## 7. Conformance
The reference decoder computes in double precision. A conforming decoder's output must be within **+-1** of the reference output on every sample and within **0.3 RMS** over each conformance stream (draft 1.2; draft 1 had +-2 / 0.5 before a fixed-point decoder existed). The integer model of section 10 meets this with margin on vectors t01-t07 plus 100 s of film, TV and full-scale stress audio (worst: every sample within 1, 4.9% of samples off by one, RMS 0.222). **Exception:** for streams whose reconstruction goes beyond full scale (vector t08, crafted: values at the +-2^26 limit; no encoder produces them) the limits are +-4 and 0.3 RMS (the integer model: 3 and 0.107), because arithmetic errors scale with the internal magnitude while the clipped output hides most of it. Conformance vectors: `tools/tac_ref/vectors/` t01-t08 with reference WAVs; `fx_expected.txt` holds the integer model's exact output (section 10). Streams must respect: allowed window sequences, `maxband` limits, `sf` in 0..255, magnitudes <= 8191, frame 0 with `reset`.

## 7b. Clarifications (normative; from the independent Python decoder review, draft 1.1)
An independent decoder written only from this text (`tools/tac_ref/py/tac_dec.py`) reproduces the reference output sample for sample on all six draft-1 vectors (and on t07 and t08 since draft 1.2) (`tools/tac_ref/vectors/`: stereo per-band, all-band and left/right modes, mono, SHORT runs, a START first frame, magnitudes up to 373). Its points of interpretation, pinned down:
1. **Container:** the `.tac` header is 20 bytes; a frame is at most 65535 bytes.
2. **Range decoder init:** of the 5 initial bytes the first is always 0 from the reference encoder and has no effect.
3. **Scale factors:** the delta is the TVC `SEG` binarisation (zero flag with `sfz[pd != 0]`, `1 + EG(sfm, 0)`, sign); `pd` is the previous delta of this channel (a zero delta gives `pd = 0`); `sf[0]` is read whenever the channel is coded, also if `maxband = 0`; scale factors are coded for every band below `maxband`, including bands that turn out all-zero.
4. **Line contexts:** `q1`, `q2` restart at 0 for each channel; lines of bands with the band-zero flag do not update them; inside a coded band every line (also magnitude 0) updates them; each non-zero magnitude's sign bin comes right after its magnitude bins.
5. **Window sequence at a start:** the first frame decoded (frame 0, or the reset frame where a decoder starts) may have any window type; afterwards the transition rules apply. A decoder starting at a reset frame other than 0 has no overlap partner for that frame's first half: it outputs from the following frame on, and needs the absolute frame index t (for the noise seed) from the container.
6. **Noise filling** applies to every eligible band below `maxband` whose decoded levels are all 0 (band-zero flag set, or coded with all lines 0); it is done per channel before the mid/side recombination; `reset` does not affect the noise seed (it depends only on t and the channel).
7. **Output rounding:** round half away from zero, then clip.
8. **Invalid streams** (a broken window sequence, `maxband` above the limit, `sf` outside 0..255, a magnitude above 8191, frame 0 without `reset`) must be rejected; the decoder stops and reports an error.
9. **Numerics:** the reference is double precision; the tolerance of section 7 is validated by the integer decoder model of section 10 (draft 1.2).

## 8. Informative: the reference encoder (draft 1)
Transient detector on the new half of each window (8 segments of 128 samples, first-difference energy above 8x the running average) selects SHORT frames with START/STOP transitions. Psychoacoustic model: band energies, spreading in Bark (-25 dB/Bark downward, -10 dB/Bark upward), a **tonality-dependent masking offset per masker band** (tonality = min(-SFM_dB / 60, 1) from the spectral flatness of the band's MDCT line energies; offset = tonality x (14.5 + z) + (1 - tonality) x 5.5 dB, z = band centre in Bark: tonal maskers mask less, so tonal content gets more bits; `-T 0` restores the fixed 13 dB offset of the first draft), absolute threshold of hearing (Terhardt) for 0 dBFS = 105 dB SPL capped at 40 dB SPL. Mid/side per band when `E_M * E_S < 0.5 * E_L * E_R`, with the stricter threshold. Per band, the largest scale factor whose quantisation error stays under the allowed noise (never so small that a magnitude exceeds 8191); a bisection on a global noise offset meets the frame's bit budget (mean + a share of a 6-frame bit reservoir); bandwidth 18.6 kHz at 128 kbit/s, 15.05 kHz at 96 kbit/s; noise filling sets all-zero bands to the band's energy. Measured on a 30 s film action excerpt at 128 kbit/s: 128.9 kbit/s, 9 SHORT frames of 1408, segmental SNR 14.5 dB, worst 20 ms segment +3.2 dB (SNR is not a perceptual measure: the listening test decides).

## 9. Open
Short-window grouping and per-window scale factors; lower rates than 96 kbit/s (needs listening). Done since draft 1: independent decoder (7b), tonality model (8), TNV3 packet mapping (`docs/TNV3_CONTAINER_V0.md` section 4.1), integer decoder model (10).

## 10. Integer decoder model (informative; the Tau hardware decoder must equal it bit for bit)
`tools/tac_ref/tac_fx.c` (`tac_fx dec|cmp|test`) reconstructs with integers only (it shares the parsing of `tac.c`). It is not normative for other decoders (section 7 is), but it is the oracle for the RTL, so every rounding is fixed here. `>>` is an arithmetic shift; `R(v, s)` = `(v + 2^(s-1)) >> s` for s > 0 (round half up), `v * 2^-s` for s <= 0. Parameters: `FX = 4` fraction bits for coefficients and the time-domain accumulator, `TW = 20` fraction bits for twiddles and windows.
1. **Tables.** `P43[i] = round(i^(4/3) * 2^16)`, i = 0..1024; `P2[b] = round(2^(b/4) * 2^22)`, b = 0..3; for n = 1024 and n = 128: pre-twiddle `(cos, sin)(pi (4j+1) / (4n))`, post-twiddle `(cos, sin)(pi j / n)` for j < n/2, FFT twiddles `(cos, sin)(2 pi j / (n/2))` for j < n/4, windows `w(n)` of section 6 (sine values, 1 and 0 exact), all `round(x * 2^TW)`. `round` = nearest, halves away from zero.
2. **Dequantiser.** `a = |q|`; `v = P43[a]` if a <= 1024, else `16 * (P43[a>>3] + R((P43[(a>>3)+1] - P43[a>>3]) * (a&7), 3))`. With `e = sf - 100`, `ex = e >> 2`, `b = e & 3`: `X = sign(q) * clamp(R(v * P2[b], 38 - FX - ex))`. Noise lines: `u = r - 32768` with r the 16-bit draw of section 6; `X = sign(u) * clamp(R(|u| * P2[b], 37 - FX - ex))`. `clamp` limits magnitudes to `2^(26+FX) - 1` (2^26 in real units, above the largest MDCT value of any 16-bit signal). Mid/side: `L = clamp(M + S)`, `R = clamp(M - S)`.
3. **Inverse transform** of n = 1024 lines (long) or each of the 8 windows of n = 128 (short), as a DCT-IV through an n/2-point complex FFT: (a) for j < n/2: `z = (X[2j] + i X[n-1-2j]) * conj(pre[j])`, stored at the bit-reversed position of j (log2(n/2) bits); (b) radix-2 decimation-in-time stages of span 2, 4, ..., n/2; in each butterfly `t = b * conj(F[k * (n/2) / span])` and the outputs are `(a + t + 1) >> 1` and `(a - t + 1) >> 1` per component; (c) for k < n/2: `v = Z[k] * conj(post[k])`, `u[2k] = Re v`, `u[n-1-2k] = -Im v`. Every complex multiply by a conjugated twiddle `(c, s)` rounds each component once: `(xr + i xi)(c - i s)` = `R(xr*c + xi*s, TW) + i R(xi*c - xr*s, TW)`. The n/2 halvings give exactly the 2/n factor of section 6, so `u` = (2/n) DCT-IV(X) in Q(FX).
4. **Unfolding and window:** output m = 0..2n-1: `y = u[m + n/2]` for m < n/2, `-u[3n/2 - 1 - m]` for m < 3n/2, else `-u[m - 3n/2]`; then `R(y * w(m), TW)` is added to the 64-bit accumulator at the frame's position (section 6; short windows at 448 + 128w).
5. **Output:** `round_half_away(acc / 2^FX)`, clipped to -32768..32767.
Widths (measured maxima): transform values 29 bits + sign on every real stream (t01-t07 and the long material), 31 bits + sign at the limit (t08), so the data path is **32-bit signed** to stay exact on every valid stream; twiddles and windows up to 2^20 (22-bit signed); transform products 54 bits before rounding. The dequantiser product `v * P2[b]` needs up to 57 bits, and up to 61 after the left shift at `sf` >= 240; any result above the limit is clamped, so a hardware dequantiser may saturate as soon as the result is known to exceed it (the output is the same). With one time-shared multiplier the long frame is 9 stages x 256 butterflies x 4 + 2 x 512 complex twiddles x 4 + 2048 window multiplies = about 15.4 k multiplies per channel and frame, about 1.44 M per second for stereo at 46.875 frames/s (arithmetic, no hardware yet). A 32 x 22 multiply does not fit one 27 x 27 DSP: the RTL needs two DSP blocks per multiplier (or a split multiply over two cycles).

**Sweep** (`tac_fx cmp`, worst case over 10 streams: the six vectors, 30 s each of film action, film dialog and a TV opening at 96 kbit/s, and a 10 s full-scale stress signal of a sine sweep, a clipped square wave and white noise; columns: largest sample difference, RMS of the integer differences, share of samples that differ, largest and RMS error of the unrounded output, data width):

| FX | TW | max diff | RMS | differing | unrounded max / RMS | data bits + sign |
|---|---|---|---|---|---|---|
| 4 | 15 | 4 | 0.673 | 35.0% | 3.52 / 0.675 | 29 |
| 4 | 16 | 2 | 0.445 | 19.5% | 1.86 / 0.339 | 29 |
| 4 | 17 | 1 | 0.317 | 10.1% | 0.92 / 0.167 | 29 |
| 4 | 18 | 1 | 0.253 | 6.4% | 0.59 / 0.105 | 29 |
| **4** | **20** | **1** | **0.222** | **4.9%** | **0.33 / 0.062** | **29** |
| 4 | 24 | 1 | 0.222 | 4.9% | 0.34 / 0.059 | 29 |
| 2 | 18 | 2 | 0.443 | 19.6% | 1.31 / 0.238 | 27 |
| 3 | 18 | 1 | 0.313 | 9.8% | 0.72 / 0.142 | 28 |
| 5 | 18 | 1 | 0.233 | 5.4% | 0.56 / 0.093 | 30 |
| 6 | 20 | 1 | 0.141 | 2.0% | 0.18 / 0.032 | 31 |
| 8 | 24 | 1 | 0.102 | 1.0% | 0.11 / 0.020 | 33 |

Findings: the twiddle precision limits below TW = 18, the accumulator fraction FX above; at FX = 4 / TW = 20 the unrounded error stays at a third of a sample, so the +-1 limit holds with margin. Two table precisions matter more than the transform: a `|q|^(4/3)` table with 8 fraction bits gave up to 7 samples of error (relative error 4e-4 at q = 3) and a `2^(b/4)` table with 14 bits gave 0.5 at full scale; both are now far below the transform's error.

Mutation test (`fx_mutants.py`): 11 changes to the model (rounding modes, table precisions, interpolation, noise sign handling, the limit, the M/S limit, output rounding, twiddle sign and precision) each change the output of at least one vector; t07 is needed for the interpolation above 1024 and t08 for both limits.

### 10b. Clarifications (from the independent Python integer model, `tools/tac_ref/py/tac_fx.py`, bit-exact on t01-t08)
1. **Windows:** LONG, START and STOP are three 2048-entry tables and the short window is one 256-entry table `ws`; START's tail (1472..1599) is the *falling* half of `ws` (entries 128..255), STOP's head (448..575) the *rising* half (0..127).
2. **Table rounding:** every table value is `round(x * 2^bits)` of the double-precision value, halves away from zero (no ties occur).
3. **Integer conventions:** `e >> 2` and `e & 3` are floor and the non-negative remainder also for negative `e` (two's complement). `R(v, s)` for `s <= 0` is a left shift, reached for `sf >= 240`. The limit is applied to the rounded magnitude (inclusive, `2^(26+FX)`), then the sign; `sign(0) = 0` (a noise draw of exactly 32768 gives 0).
4. **Order:** dequantisation and noise filling per channel (noise in band order and coding order, section 6 / 7b.6), then mid/side with the limit, then the transform.
5. **Butterfly layout:** in a stage of span `m` (group size; 2, 4, ..., n/2), for each group start `g` (multiple of m) and k < m/2: `a = Z[g+k]`, `b = Z[g+k+m/2]`, `t = b * conj(F[k * (n/2) / m])`, `Z[g+k] = (a + t + 1) >> 1`, `Z[g+k+m/2] = (a - t + 1) >> 1`, per component. The input permutation is `Z[bitrev(j)] = z[j]`.
6. **Complex products** form each component's sum at full precision and round once (`R(xr*c + xi*s, TW)`), not each product.
7. **Two roundings:** `R` (half up) inside the model, round half away from zero only for the final sample (`(|acc| + 8) >> 4` with the sign of acc).

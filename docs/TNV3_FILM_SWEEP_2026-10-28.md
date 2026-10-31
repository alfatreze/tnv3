# TNV3 whole-film sweep (2026-10-28)

First complete TNV3 files made from whole films with the reference tools (`tools/tnv3_ref/encode_film.py`, `film_sweep.py`): TVC video at QP 26 / 30 / 34, TAC audio at 96 kbit/s, muxed with chunk CRCs and validated by `tnv3 check`. Quality is measured per 480-frame segment (VMAF 0.6.1, PSNR-Y, SSIM-Y against the scaled source) and averaged by frames. Hardware: this Mac, 12 encoder processes in parallel. Reports and the three QP 30 files: `tau-nu/work/films/` (git-ignored: `*_q30.tnv3`, per-run `*.json`, `sweep.json`); the numbers are reproduced here.

## 1. Results
| Film (source -> TNV3) | QP | VMAF (worst 20 s) | PSNR-Y | SSIM-Y | video kbit/s | **MB per hour** (incl. 96k audio) | worst second (limit) | largest picture | pictures re-encoded for the level | encode time (x real time) |
|---|---|---|---|---|---|---|---|---|---|---|
| Tears of Steel, 1920x800 24 fps -> 800x334, Level 2 (730 s) | 26 | 96.5 (92.7) | 41.5 | 0.984 | 1224 | 597 | 413 kB (466 kB) | 92 kB | 149, 3 joints | 1190 s (1.63x) |
| | **30** | **93.8** (88.7) | 39.1 | 0.976 | 737 | **378** | 408 kB | 78 kB | 0 | 460 s (0.63x) |
| | 34 | 88.5 (82.0) | 36.5 | 0.961 | 428 | 239 | 253 kB | 62 kB | 0 | 429 s (0.59x) |
| B&W TV episode, 640x480 23.98 fps -> native 640x480 at 24000/1001, Level 3 (1,543 s) | 26 | 95.2 (91.4) | 42.9 | 0.984 | 912 | 457 | 409 kB (666 kB) | 90 kB | 0 | 1129 s (0.73x) |
| | **30** | **91.5** (89.2) | 40.5 | 0.975 | 551 | **294** | 261 kB | 71 kB | 0 | 1101 s (0.71x) |
| | 34 | 85.1 (80.9) | 38.0 | 0.959 | 325 | 193 | 162 kB | 51 kB | 0 | 959 s (0.62x) |
| Anime episode, 1920x1080 23.976 fps -> 800x450, Level 2 (1,440 s) | 26 | 95.9 (93.4) | 46.6 | 0.990 | 480 | 251 | 414 kB (466 kB) | 158 kB | 209, 3 joints | 1329 s (0.92x) |
| | **30** | **94.4** (90.2) | 44.5 | 0.986 | 327 | **182** | 412 kB | 121 kB | 22, 1 joint | 1023 s (0.71x) |
| | 34 | 91.7 (84.9) | 42.0 | 0.978 | 217 | 132 | 350 kB | 89 kB | 0 | 873 s (0.61x) |

**Size at VMAF 90** (interpolated between QPs): Tears of Steel **278 MB/h**, B&W TV **271 MB/h**, anime about **100 MB/h** (extrapolated: even QP 34 is at VMAF 91.7). Audio is 43 MB/h of that (96 kbit/s + container). All nine files are valid; the B&W episode's 1199/50 fps source was snapped to 24000/1001.

## 2. Findings
1. **The size budget holds with room**: the owner's 400-450 MB/h at 800x450 is met at VMAF 90 by every content type (100-280 MB/h), and QP 30 (VMAF 91-94) costs 180-380 MB/h. A 2-hour film at QP 30 is about 0.4-0.8 GB.
2. **One QP does not fit all content**: VMAF at QP 30 spans 91.5-94.4 and the size 182-378 MB/h. A rate-control mode that targets a quality (or a per-title QP search) belongs in the production encoder.
3. **The level limits bite only at high quality**: at QP 26 the Level 2 one-second window (400 kB of video) forced 149 (Tears) and 209 (anime) pictures to a higher QP and 3 segment re-encodes at joints; worst seconds came within 11% of the 466 kB chunk limit. QP 30 needed 22 adjustments on anime (scene changes with large I pictures) and none elsewhere. The limits work as designed (no file failed); quality-targeted encodes near the limit will see occasional QP raises on hard seconds.
4. **Worst 20-second segments** are 3-7 VMAF below the film average at every QP (dark, high-motion or grainy scenes), consistent with a fixed QP; the look-ahead QP already in the encoder does not address it, a quality-targeted mode would.
5. **Encode speed**: the reference encoder runs at 0.6-0.9x real time on 12 cores at QP 30-34 (1.6x at QP 26 on Tears): about 1.3-1.9 hours for a 2-hour film. Speed work (SIMD, faster decisions, presets) is an encoder task for Tau Omega.
6. Memory load of these files on a hardware decoder: `docs/TNV3_MEMORY_STUDY_2026-10-28.md` (the QP 30 files were traced).

## 3. Reproduce
`python3 tools/tnv3_ref/film_sweep.py OUTDIR` (needs `tau-nu/media/`: tears-of-steel.mp4, "MPEG-4 movie.mp4", the anime .mkv; about 2.5 hours on this machine). Single film: `python3 tools/tnv3_ref/encode_film.py SOURCE OUT.tnv3 --qp 30 --quality`.

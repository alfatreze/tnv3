# R0 intra share: how much of a film's size goes to intra coding

Method: three 60 s segments per source, scaled with `fps=24,scale=WxH:flags=lanczos,format=yuv420p` to a lossless y4m; libx264 `-preset slow -crf 24`, params `partitions=none:8x8dct=0:ref=1:weightp=0:mbtree=0:bframes=3:b-pyramid=normal:b-adapt=0:weightb=0:cabac=1` with keyint/min-keyint/scenecut per variant (A 48/24/40, B 96/24/40, C 240/24/40, D 96/96/0).
Cuts: ffmpeg `scdet` threshold 10 on the y4m. Frame counts and byte totals per picture type come from ffprobe `-show_frames` (pkt_size); intra MB % in P/B from the x264 summary (I16 + I4 + I8 columns of the `mb P`/`mb B` lines).
I-frame bytes include the whole picture. 'Est. total intra' = I-frame bytes + intra MB % x (P bytes) + intra MB % x (B bytes), divided by all bytes. It is an ESTIMATE and a lower bound (intra MBs are larger than the average MB of their frame). 'Extra I' = I frames beyond ceil(frames/K).
Source: x264 via ffmpeg 9.0.2; source segment offsets as listed per row.

## tears

| segment (s) | cuts | var | I frames | extra I | I byte share | mean I | mean P | mean B | intra MB% P | intra MB% B | est. total intra | mean B/frame |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 120 | 7 | A | 33 | 3 | 20.9% | 22456 | 4604 | 1088 | 3.3 | 0.1 | 22.5% | 2461 |
| 120 | 7 | B | 18 | 3 | 12.7% | 23507 | 4776 | 1113 | 3.4 | 0.1 | 14.5% | 2311 |
| 120 | 7 | C | 10 | 4 | 7.8% | 25138 | 4872 | 1126 | 3.5 | 0.1 | 9.8% | 2232 |
| 120 | 7 | D | 15 | 0 | 9.9% | 22201 | 5085 | 1120 | 5.1 | 0.1 | 12.7% | 2331 |
| 300 | 20 | A | 42 | 12 | 21.6% | 22096 | 5308 | 1386 | 4.5 | 0.5 | 23.8% | 2981 |
| 300 | 20 | B | 25 | 10 | 14.3% | 22951 | 5410 | 1410 | 4.0 | 0.5 | 16.4% | 2790 |
| 300 | 20 | C | 19 | 13 | 11.8% | 24584 | 5444 | 1424 | 4.1 | 0.5 | 14.1% | 2743 |
| 300 | 20 | D | 15 | 0 | 8.4% | 22898 | 6094 | 1447 | 8.0 | 0.6 | 13.0% | 2832 |
| 480 | 22 | A | 52 | 22 | 16.9% | 20990 | 7830 | 2414 | 18.0 | 2.4 | 25.9% | 4484 |
| 480 | 22 | B | 30 | 15 | 9.5% | 19464 | 8066 | 2433 | 18.9 | 2.3 | 19.8% | 4255 |
| 480 | 22 | C | 26 | 20 | 8.3% | 19322 | 8006 | 2447 | 18.5 | 2.4 | 18.5% | 4196 |
| 480 | 22 | D | 15 | 0 | 5.5% | 22467 | 8488 | 2568 | 22.3 | 3.2 | 18.0% | 4256 |

## anime

| segment (s) | cuts | var | I frames | extra I | I byte share | mean I | mean P | mean B | intra MB% P | intra MB% B | est. total intra | mean B/frame |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 200 | 44 | A | 54 | 24 | 28.5% | 36553 | 8055 | 1737 | 12.3 | 1.1 | 34.6% | 4802 |
| 200 | 44 | B | 40 | 25 | 22.2% | 36027 | 8372 | 1734 | 12.5 | 1.1 | 28.8% | 4517 |
| 200 | 44 | C | 36 | 30 | 18.7% | 32460 | 8331 | 1809 | 12.2 | 1.2 | 25.4% | 4342 |
| 200 | 44 | D | 15 | 0 | 10.6% | 44326 | 8635 | 2339 | 14.3 | 2.5 | 18.7% | 4351 |
| 700 | 17 | A | 37 | 7 | 43.7% | 40062 | 3309 | 682 | 3.2 | 0.4 | 44.9% | 2356 |
| 700 | 17 | B | 23 | 8 | 33.7% | 43751 | 3430 | 691 | 3.4 | 0.4 | 35.3% | 2071 |
| 700 | 17 | C | 16 | 10 | 28.1% | 46515 | 3251 | 681 | 3.3 | 0.4 | 29.7% | 1841 |
| 700 | 17 | D | 15 | 0 | 23.2% | 45749 | 3870 | 826 | 6.0 | 0.6 | 26.2% | 2055 |
| 1200 | 26 | A | 49 | 19 | 26.7% | 29813 | 6484 | 1554 | 11.5 | 1.0 | 32.1% | 3800 |
| 1200 | 26 | B | 30 | 15 | 18.5% | 31108 | 6523 | 1598 | 11.8 | 1.0 | 24.6% | 3502 |
| 1200 | 26 | C | 25 | 19 | 14.0% | 27080 | 6567 | 1647 | 11.7 | 1.1 | 20.3% | 3360 |
| 1200 | 26 | D | 15 | 0 | 11.8% | 39578 | 7138 | 1736 | 14.1 | 1.6 | 19.7% | 3481 |

## bw

| segment (s) | cuts | var | I frames | extra I | I byte share | mean I | mean P | mean B | intra MB% P | intra MB% B | est. total intra | mean B/frame |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 200 | 11 | A | 39 | 9 | 17.8% | 34165 | 9368 | 2665 | 5.3 | 0.3 | 20.3% | 5204 |
| 200 | 11 | B | 22 | 7 | 11.3% | 36373 | 9646 | 2677 | 5.1 | 0.3 | 13.9% | 4939 |
| 200 | 11 | C | 15 | 9 | 8.4% | 39264 | 9736 | 2698 | 5.2 | 0.3 | 11.2% | 4843 |
| 200 | 11 | D | 15 | 0 | 7.0% | 33505 | 10029 | 2907 | 7.8 | 0.3 | 11.0% | 5007 |
| 900 | 8 | A | 37 | 7 | 16.3% | 24442 | 7084 | 1997 | 6.6 | 0.7 | 19.6% | 3849 |
| 900 | 8 | B | 18 | 3 | 9.3% | 26814 | 7283 | 1967 | 6.0 | 0.9 | 12.7% | 3614 |
| 900 | 8 | C | 10 | 4 | 6.1% | 31106 | 7429 | 1944 | 5.7 | 1.0 | 9.6% | 3525 |
| 900 | 8 | D | 15 | 0 | 6.6% | 23117 | 7436 | 2094 | 8.8 | 0.6 | 11.3% | 3649 |
| 1481 | 0 | A | 33 | 3 | 24.2% | 33984 | 6562 | 1089 | 5.2 | 0.2 | 26.9% | 3219 |
| 1481 | 0 | B | 17 | 2 | 13.7% | 34204 | 6704 | 1147 | 5.4 | 0.2 | 16.8% | 2958 |
| 1481 | 0 | C | 10 | 4 | 6.3% | 25396 | 6848 | 1188 | 5.2 | 0.2 | 9.6% | 2787 |
| 1481 | 0 | D | 15 | 0 | 13.0% | 36441 | 6780 | 1129 | 5.1 | 0.3 | 16.1% | 2909 |

## Cuts per minute (scdet threshold 10, mean of the segments)

| source | cuts per segment | cuts/min |
|---|---|---|
| tears | 7, 20, 22 | 16.3 |
| anime | 44, 17, 26 | 29.0 |
| bw | 11, 8, 0 | 6.3 |
| all | | 17.2 |

## Summary: mean over all segments

| var | segments | cuts/min | I byte share | extra I frames | intra MB% P | intra MB% B | est. total intra | mean B/frame |
|---|---|---|---|---|---|---|---|---|
| A | 9 | 17.2 | 24.1% | 11.8 | 7.8 | 0.7 | 27.8% | 3684 |
| B | 9 | 17.2 | 16.1% | 9.8 | 7.8 | 0.8 | 20.3% | 3440 |
| C | 9 | 17.2 | 12.2% | 12.6 | 7.7 | 0.8 | 16.4% | 3319 |
| D | 9 | 17.2 | 10.7% | 0.0 | 10.2 | 1.1 | 16.3% | 3430 |

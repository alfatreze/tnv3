# 001: Rust encoder can starve its rolling rate window

**Status:** open; host-observed 2026-11-01.

The normal Rust TVC encoder can consume most of a level's rolling picture-byte
window on earlier frames, then return `Picture cannot fit TVC level limits`
even after retrying the current frame through QP 51. The experimental B stream
can similarly return `Picture cannot fit TVC rate limits`. Expected: a valid
encode or a planned, quality-aware way to allocate the window before it becomes
unrecoverable. Impact: some otherwise supported films cannot complete.

Reproduction: use the local `film-input.mkv` or `film-800-input.mkv` under
`local-media/work/encoder_lab/`, decode the first 48 frames to Y4M padded to
a multiple of 16, and encode at QP 24. Both modes fail at 400×176 and 800×336;
QP 30 also fails at 800×336. Aggregate host evidence is in
`docs/rust_long_rate_study.json`; local media is not published.

Workaround: build with Cargo feature `safe-rate` (`make rust-safe-rate` for
the P stream). This fixed these test cases by limiting every picture to an
equal share of the rolling budget, but may lower quality or still fail at QP
51. Next: implement bounded look-ahead or segment-aware allocation, measure
quality and ensure windows remain valid across segment boundaries. No hardware
validation is implied.

2026-11-01 follow-up (**host**): raising the equal-share picture cap to 1.5×
recovered quality for the 400-pixel QP 30 B case, but reproduced all three
previous failure combinations in both P and B modes. The relaxed cap was
rejected; see `docs/RUST_ENCODER_EXPERIMENTS.md` and
`docs/rust_long_rate_study.json` for the measurements.

2026-11-01 follow-up (**host**): trying a rate-compliant predicted picture
after an oversized QP-51 intra candidate did not rescue the failing film cases.
The scratch-only approach was not shipped; see the experiment record.

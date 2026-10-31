# Release candidate validation

Source snapshot: Tau V encoder-lab `637b9f83dbee2807b5fee382dfdf28dca2a007d5`,
plus the isolated release-integration changes identified in SOURCE_MANIFEST.json.

On macOS ARM64, the standalone build and synthetic smoke test pass: the serial
and experimental slice encoders produce identical level-3 streams, their
reconstructions match the frozen reference decoder, and TAC/TNV3 mux and check
succeed. TAC reference and fixed-point model decode the synthetic audio with the
expected 48,000 stereo sample frames; container demux and audio decoding are
also checked. The fixed-point transform self-test passes. Serial fallback is exercised when one worker is requested or pool
initialization is forced to fail. A separately generated 24-frame 800x480 test
with silence passes the full film helper at level 3; serial and parallel TNV3
files match byte-for-byte. This is a generated study input, not a feature film.

The extracted serial encoder also passed the existing 4-clip, 3-QP corpus gate:
mean BD-rate −1.52% versus the frozen original, mean bins/byte 9.676 versus
9.618, bins/macroblock 51.87 versus 52.13. Full prior evidence and methodology
are recorded in the source worktree's encoder audit; the two included encoder
reports summarize the results. The experimental slice encoder was compared
against serial at the same slice layout; introducing extra slices by default
failed the quality gate. The reported 1.82x speedup is for an already-required
level-3 two-slice 800-pixel study case, not a full-length-film benchmark.

The standalone synthetic smoke test also passed on GitHub-hosted Ubuntu and
macOS runners for commit 15aade93b8a389f2da7d61ca6e12cd3cf8b68ad3
([CI run](https://github.com/alfatreze/tnv3/actions/runs/41214788520)). Full
Linux corpus/performance gates and native Windows tests remain open, as do all
allocation failures, long films, power/thermal limits and new Pocket testing. The
experimental Windows threading backend has only been exercised through a model.
No GPU/ANE acceleration is enabled. All formats remain draft specifications.

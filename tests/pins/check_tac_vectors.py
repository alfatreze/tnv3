#!/usr/bin/env python3
"""Run the C TAC decoders over the pinned Tau V vectors t01-t08 (read in place).

`tac dec` must match the reference WAV exactly; `tac_fx dec` (integer model) must stay within
+-1 sample (+-4 for t08, spec section 7). Verify the pins first: tests/pins/check_pins.py.
"""
import glob, os, subprocess, sys, tempfile, wave, array, math
here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.normpath(os.path.join(here, "..", ".."))
root = os.environ.get("TAU_V_ROOT") or os.path.normpath(os.path.join(repo, "..", "tau-nu/.claude/worktrees/codec-alternatives"))
vec = os.path.join(root, "tools/tac_ref/vectors")
tools = {k: os.path.join(repo, "tools/tac_ref", k) for k in ("tac", "tac_fx")}
files = sorted(glob.glob(os.path.join(vec, "t0*.tac")))
if len(files) != 8:
    sys.exit(f"expected 8 vectors in {vec}, found {len(files)}")
def pcm(path):
    w = wave.open(path); d = array.array("h", w.readframes(w.getnframes())); return w.getparams()[:3], d
bad = 0
with tempfile.TemporaryDirectory() as tmp:
    for f in files:
        name = os.path.basename(f)[:-4]
        refp, ref = pcm(f[:-4] + ".ref.wav")
        for model, limit in (("tac", 0), ("tac_fx", 4 if name.startswith("t08") else 1)):
            out = os.path.join(tmp, name + model + ".wav")
            subprocess.run([tools[model], "dec", f, out], check=True, capture_output=True)
            p, d = pcm(out)
            diffs = [abs(a - b) for a, b in zip(ref, d)]
            mx = max(diffs, default=0); rms = math.sqrt(sum(v * v for v in diffs) / max(len(diffs), 1))
            ok = p == refp and len(d) == len(ref) and mx <= limit and (model != "tac_fx" or rms <= 0.3 + (0 if limit == 1 else 0))
            bad += not ok
            print(f"{'ok  ' if ok else 'FAIL'} {name} {model:6} max {mx} rms {rms:.3f} limit {limit}")
sys.exit(1 if bad else 0)

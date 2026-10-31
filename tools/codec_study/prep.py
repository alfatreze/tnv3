#!/usr/bin/env python3
"""Cut the study clips from their sources and write lossless y4m references.

Clips are exactly 240 frames at 24 fps.  The 800-wide and 400-wide versions are both derived
directly from the source with Lanczos (the 400 clip is NOT a downscale of the 800 clip, so the
resolution-switch test is fair to both).  See clips.py for the list.  Clips with `src2` are cut from
already-scaled lossless intermediates (the anime episode, see slime_intermediate.py).
"""
import pathlib, subprocess, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from clips import CLIPS

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
FRAMES = 240


def main():
    ref = ROOT / "ref"
    ref.mkdir(parents=True, exist_ok=True)
    bad = 0
    for name, c in CLIPS.items():
        for tag, (w, h) in (("800", (c["w"], c["h"])), ("400", (c["w2"], c["h2"]))):
            out = ref / f"{name}_{tag}.y4m"
            if out.exists():
                continue
            src = c["src2"] if tag == "400" and "src2" in c else c["src"]
            p = subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(c["ss"]), "-i", str(src), "-map", "0:v:0",
                                "-an", "-vf", f"fps=24,scale={w}:{h}:flags=lanczos,format=yuv420p",
                                "-frames:v", str(FRAMES), "-f", "yuv4mpegpipe", str(out)], capture_output=True, text=True)
            msgs = [ln for ln in p.stderr.splitlines() if ln.strip()]
            fsz = w * h + 2 * ((w + 1) // 2) * ((h + 1) // 2) + 6
            n = out.stat().st_size / fsz
            print(f"{out.name}: {n:.0f} frames, {len(msgs)} decoder messages", msgs[:2])
            if msgs or abs(n - FRAMES) > 1:
                bad += 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())

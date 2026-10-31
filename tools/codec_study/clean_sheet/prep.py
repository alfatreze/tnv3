import subprocess, pathlib, sys
S = pathlib.Path(__file__).parent
M = "<tau-v-worktree>/media/tears-of-steel.mp4"
(S / "clips").mkdir(exist_ok=True)
for n, t in [("dialog", 355), ("dark", 115), ("action", 436)]:
    out = S / "clips" / f"{n}.y4m"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(t), "-i", M, "-frames:v", "48",
                    "-vf", "scale=800:334:flags=lanczos", "-pix_fmt", "yuv420p", str(out)], check=True)
    print(out, out.stat().st_size)
for e in ["jpeg2000", "vc2", "cfhd", "snow"]:
    r = subprocess.run(["ffmpeg", "-hide_banner", "-h", f"encoder={e}"], capture_output=True, text=True)
    print("=====", e)
    print("\n".join(l for l in (r.stdout + r.stderr).splitlines() if l.strip())[:1800])
r = subprocess.run(["ffmpeg", "-hide_banner", "-decoders"], capture_output=True, text=True)
print([l for l in r.stdout.splitlines() if any(k in l.lower() for k in ("av1", "dirac", "snow", "vp9"))])

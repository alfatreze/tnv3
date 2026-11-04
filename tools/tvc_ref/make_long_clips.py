"""Cut three 60 s test segments with real scene cuts (lossless y4m, 1440 frames at 24 fps) for picture-type and
scene-cut tests: Tears of Steel at 300 s (800x334), the anime intermediate at 700 s (800x450), the B&W episode at 900 s
(800x600).  Output: OUT_DIR/long_{tears,anime,bw}.y4m.  Usage: python3 make_long_clips.py OUT_DIR"""
import pathlib, subprocess, sys
MEDIA = pathlib.Path("<tau-v-checkout>/media")
WORK = pathlib.Path("<tau-v-checkout>/work/codec_phase0")
SEGS = [("tears", MEDIA / "tears-of-steel.mp4", 300, 800, 334), ("anime", WORK / "slime_800.mkv", 700, 800, 450),
        ("bw", MEDIA / "MPEG-4 movie.mp4", 900, 800, 600)]
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
for name, src, ss, w, h in SEGS:
    dst = out / f"long_{name}.y4m"
    if dst.exists(): continue
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(ss), "-i", str(src), "-map", "0:v:0", "-an",
                    "-vf", f"fps=24,scale={w}:{h}:flags=lanczos,format=yuv420p", "-frames:v", "1440", "-f", "yuv4mpegpipe", str(dst)], check=True)
    print(dst, dst.stat().st_size)

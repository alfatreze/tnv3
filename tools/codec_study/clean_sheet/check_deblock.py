"""Which ffmpeg/x264 option really switches the deblocking filter off?  Reads x264's settings SEI from each encode."""
import subprocess, re, pathlib, tempfile
REF = "<study-worktree>/work/codec_phase0/ref/dialog_800.y4m"
VARIANTS = {
    "default": [],
    "x264-params no-deblock=1": ["-x264-params", "no-deblock=1"],
    "x264-params deblock=0": ["-x264-params", "deblock=0"],
    "flags -loop": ["-flags", "-loop"],
    "x264opts no-deblock": ["-x264opts", "no-deblock"],
}
tmp = pathlib.Path(tempfile.mkdtemp())
for name, extra in VARIANTS.items():
    out = tmp / "o.h264"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", REF, "-frames:v", "24", "-c:v", "libx264", "-preset", "slow",
                    *extra, "-crf", "28", str(out)], check=True)
    data = out.read_bytes()
    m = re.search(rb"deblock=[0-9:-]+", data)
    print(f"{name:28s} {m.group().decode() if m else '?':16s} {len(data)} bytes")

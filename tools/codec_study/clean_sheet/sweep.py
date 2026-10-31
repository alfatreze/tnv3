import subprocess, pathlib, json, re, sys, os
from concurrent.futures import ThreadPoolExecutor
S = pathlib.Path(__file__).parent
CL = S / "clips"
OUT = S / "out"; OUT.mkdir(exist_ok=True)
CLIPS = ["dialog", "dark", "action"]
NF = 48

def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)

def ff(*a):
    return ["ffmpeg", "-v", "error", "-y", *a]

def enc(codec, q, clip, f):
    src = str(CL / f"{clip}.y4m")
    if codec == "aom_intra":
        return run(["aomenc", "--cpu-used=4", "--end-usage=q", f"--cq-level={q}", f"--min-q={max(0,q-6)}", f"--max-q={min(63,q+6)}",
                    "--kf-max-dist=0", "--lag-in-frames=0", "--threads=1", "-o", f, src])
    if codec == "aom_ld":
        return run(["aomenc", "--cpu-used=4", "--end-usage=q", f"--cq-level={q}", "--min-q=1", f"--max-q=63",
                    "--kf-max-dist=96", "--lag-in-frames=0", "--threads=1", "-o", f, src])
    if codec == "x264_intra":
        return run(ff("-i", src, "-c:v", "libx264", "-preset", "slow", "-g", "1", "-crf", str(q), f))
    if codec == "x264_ld":
        return run(ff("-i", src, "-c:v", "libx264", "-preset", "slow", "-bf", "0", "-g", "96", "-crf", str(q), f))
    if codec == "vp9_intra":
        return run(ff("-i", src, "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", str(q), "-g", "1", "-deadline", "good", "-cpu-used", "2", f))
    if codec.startswith("lite_") or codec == "x264_i16":  # tool-restricted x264
        base = "partitions=none:8x8dct=0:ref=1:weightp=0:mbtree=0:scenecut=0:keyint=96:min-keyint=96:bframes=0:cabac=1"
        extra = {"lite_cabac": "", "lite_cavlc": ":cabac=0", "lite_nodb": ":no-deblock=1",
                 "lite_b": ":bframes=3:b-pyramid=normal:b-adapt=0:weightb=0", "lite_cavlc_nodb": ":cabac=0:no-deblock=1",
                 "x264_i16": ":keyint=1:min-keyint=1",
                 "lite_db0": ":deblock=0", "lite_8x8": ":8x8dct=1:partitions=i8x8", "lite_part": ":partitions=p8x8",
                 "lite_i4": ":partitions=i4x4", "lite_ref3": ":ref=3", "lite_mbtree": ":mbtree=1:rc-lookahead=40",
                 "lite_all": ":8x8dct=1:partitions=all:ref=3:mbtree=1:rc-lookahead=40:weightp=2"}[codec]
        return run(ff("-i", src, "-c:v", "libx264", "-preset", "slow", "-x264-params", base + extra, "-crf", str(q), f))
    if codec == "mpeg2_b":
        return run(ff("-i", src, "-c:v", "mpeg2video", "-g", "96", "-bf", "2", "-q:v", str(q), "-mbd", "rd", "-trellis", "1", "-pix_fmt", "yuv420p", f))
    if codec == "mpeg2_intra":
        return run(ff("-i", src, "-c:v", "mpeg2video", "-g", "1", "-q:v", str(q), "-mbd", "rd", "-trellis", "1", "-intra_vlc", "1", "-pix_fmt", "yuv420p", f))
    if codec == "mpeg2_ld":
        return run(ff("-i", src, "-c:v", "mpeg2video", "-g", "96", "-bf", "0", "-q:v", str(q), "-mbd", "rd", "-trellis", "1", "-pix_fmt", "yuv420p", f))
    if codec == "mjpeg":
        return run(ff("-i", src, "-c:v", "mjpeg", "-huffman", "optimal", "-q:v", str(q), "-pix_fmt", "yuvj420p", f))
    if codec.startswith("vc2"):
        wt = {"vc2_97": "9_7", "vc2_53": "5_3", "vc2_haar": "haar"}[codec]
        return run(ff("-i", src, "-c:v", "vc2", "-wavelet_type", wt, "-wavelet_depth", "4", "-slice_width", "32",
                      "-slice_height", "16", "-b:v", f"{q}k", "-tolerance", "10", f))
    if codec.startswith("j2k"):
        pred = "0" if codec == "j2k_97" else "1"
        return run(ff("-i", src, "-c:v", "jpeg2000", "-pred", pred, "-format", "j2k", "-layer_rates", str(q), "-pix_fmt", "yuv420p", f))
    raise SystemExit(codec)

def dec(codec, f, d):
    return run(ff("-i", f, "-pix_fmt", "yuv420p", d))

def score(d, clip):
    r = run(["ffmpeg", "-hide_banner", "-i", d, "-i", str(CL / f"{clip}.y4m"), "-lavfi",
             "[0:v]split[a][b];[1:v]split[c][e];[a][c]libvmaf=n_threads=2;[b][e]psnr", "-f", "null", "-"])
    t = r.stderr
    v = re.search(r"VMAF score: ([0-9.]+)", t)
    p = re.search(r"PSNR y:([0-9.]+)", t)
    return (float(v.group(1)) if v else None, float(p.group(1)) if p else None)

def job(args):
    codec, q, clip = args
    ext = "ivf" if codec.startswith("aom") else "nut"
    base = f"{codec}_{q}_{clip}"
    f = str(OUT / f"{base}.{ext}")
    d = str(OUT / f"{base}.dec.y4m")
    try:
        e = enc(codec, q, clip, f)
        if not os.path.exists(f):
            return dict(codec=codec, q=q, clip=clip, err="enc " + (e.stderr or "")[-200:])
        size = os.path.getsize(f)
        r = dec(codec, f, d)
        if not os.path.exists(d):
            return dict(codec=codec, q=q, clip=clip, err="dec " + (r.stderr or "")[-200:])
        v, p = score(d, clip)
        os.remove(d)
        return dict(codec=codec, q=q, clip=clip, bytes=size, kb_frame=size / NF / 1000, vmaf=v, psnr=p)
    except Exception as ex:
        return dict(codec=codec, q=q, clip=clip, err=str(ex))

SW = {
    "aom_intra": [28, 34, 40, 44, 50],
    "aom_ld": [24, 32, 40, 48, 56],
    "x264_intra": [16, 20, 24, 28, 32],
    "x264_ld": [20, 24, 28, 32, 36],
    "vp9_intra": [44, 50, 54, 58, 62],
    "mjpeg": [4, 8, 12, 18, 24],
    "mpeg2_intra": [2, 4, 7, 11, 16],
    "mpeg2_ld": [2, 4, 7, 11, 16],
    "mpeg2_b": [2, 4, 7, 11, 16],
    "lite_cabac": [20, 24, 28, 32, 36],
    "lite_cavlc": [20, 24, 28, 32, 36],
    "lite_nodb": [20, 24, 28, 32, 36],
    "lite_cavlc_nodb": [20, 24, 28, 32, 36],
    "lite_b": [20, 24, 28, 32, 36],
    "x264_i16": [16, 20, 24, 28, 32],
    "lite_db0": [20, 24, 28, 32, 36], "lite_8x8": [20, 24, 28, 32, 36], "lite_part": [20, 24, 28, 32, 36],
    "lite_i4": [20, 24, 28, 32, 36], "lite_ref3": [20, 24, 28, 32, 36], "lite_mbtree": [20, 24, 28, 32, 36],
    "lite_all": [20, 24, 28, 32, 36],
                "j2k_97": [20, 40, 70, 110, 170],
    "j2k_53": [20, 40, 70, 110, 170],
}
if __name__ == "__main__":
    sel = sys.argv[1:] or list(SW)
    jobs = [(c, q, cl) for c in sel for q in SW[c] for cl in CLIPS]
    res = []
    with ThreadPoolExecutor(10) as ex:
        for r in ex.map(job, jobs):
            res.append(r)
    with open(S / "results_" + "_".join(sel)[:40] + ".json" if False else S / "results.json", "w") as fh:
        json.dump(res, fh)
    bad = [r for r in res if "err" in r]
    print(len(res), "runs;", len(bad), "errors")
    for r in bad[:12]:
        print(r)

import subprocess, pathlib, json, re, sys, os, time
from concurrent.futures import ThreadPoolExecutor
S = pathlib.Path(__file__).parent
CL = pathlib.Path("<study-worktree>/work/codec_phase0/ref")
OUT = pathlib.Path("/private/tmp/claude-501/-Users-abel-santos-Downloads-DEV-PROJECTS-Tau-Alpha-tau-nu/6ee0a67e-dbcf-4484-9eb3-cee8ffa7a25e/scratchpad/r0out"); OUT.mkdir(parents=True, exist_ok=True)
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]
import threading
LOCK = threading.Lock()
NF = 240

def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)

def ff(*a):
    return ["ffmpeg", "-v", "error", "-y", *a]

def enc(codec, q, clip, f):
    src = str(CL / f"{clip}_800.y4m")
    if codec == "aom_intra":
        return run(["aomenc", "--cpu-used=4", "--end-usage=q", f"--cq-level={q}", f"--min-q={max(0,q-6)}", f"--max-q={min(63,q+6)}",
                    "--kf-max-dist=0", "--lag-in-frames=0", "--threads=1", "-o", f, src])
    if codec == "aom_ld6":
        return run(["aomenc", "--cpu-used=6", "--end-usage=q", f"--cq-level={q}", "--min-q=1", f"--max-q=63",
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
    r = run(["ffmpeg", "-hide_banner", "-i", d, "-i", str(CL / f"{clip}_800.y4m"), "-lavfi",
             "[0:v]split=3[a][b][s];[1:v]split=3[c][e][t];[a][c]libvmaf=n_threads=2;[b][e]psnr;[s][t]ssim", "-f", "null", "-"])
    t = r.stderr
    v = re.search(r"VMAF score: ([0-9.]+)", t)
    p = re.search(r"PSNR y:([0-9.]+)", t)
    m = re.search(r"SSIM Y:\S+ \(\S+\) U:\S+ \(\S+\) V:\S+ \(\S+\) All:([0-9.]+)", t)
    return (float(v.group(1)) if v else None, float(p.group(1)) if p else None, float(m.group(1)) if m else None)

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
        v, p, ss = score(d, clip)
        os.remove(d); os.remove(f)
        if v is None or p is None or ss is None:
            return dict(codec=codec, q=q, clip=clip, err="score missing", bytes=size)
        return dict(codec=codec, q=q, clip=clip, bytes=size, kb_frame=size / NF / 1000, vmaf=v, psnr=p, ssim=ss)
    except Exception as ex:
        return dict(codec=codec, q=q, clip=clip, err=str(ex))

X = [18, 22, 26, 30, 34, 38]
SW = {
    "x264_ld": X, "mpeg2_ld": [2, 3, 5, 8, 12, 18], "mpeg2_intra": [2, 4, 6, 9, 13, 19],
    "x264_intra": [14, 18, 22, 26, 30, 34],
    "lite_cabac": X, "lite_cavlc": X, "lite_nodb": X, "lite_cavlc_nodb": X, "lite_b": X,
    "lite_8x8": X, "lite_mbtree": X, "lite_all": X,
    "aom_ld6": [20, 28, 36, 44, 52, 60],
}
RES = S / "r0_results.json"

if __name__ == "__main__":
    args = sys.argv[1:]
    pilot = "--pilot" in args
    sel = [a for a in args if not a.startswith("--")] or list(SW)
    res = json.load(open(RES)) if RES.exists() and not pilot else []
    done = {(r["codec"], r["q"], r["clip"]) for r in res if "err" not in r}
    res = [r for r in res if "err" not in r]
    clips = ["dialog"] if pilot else CLIPS
    jobs = [(c, q, cl) for cl in clips for c in sel for q in (SW[c][:1] + SW[c][-1:] if pilot else SW[c]) if (c, q, cl) not in done]
    print(len(jobs), "jobs", flush=True)
    t0 = time.time(); n = 0
    def wrapped(j):
        r = job(j)
        global n
        with LOCK:
            res.append(r); n += 1
            if not pilot:
                json.dump(res, open(RES, "w"))
            print(n, len(jobs), j, round(time.time() - t0), r.get("vmaf"), r.get("err", "")[:80], flush=True)
        return r
    with ThreadPoolExecutor(12) as ex:
        list(ex.map(wrapped, jobs))
    bad = [r for r in res if "err" in r]
    print(len(res), "runs;", len(bad), "errors", round(time.time() - t0), "s")
    for r in bad[:12]:
        print(r)
    if pilot:
        for r in sorted(res, key=lambda r: (r["codec"], r["q"])):
            print(r)

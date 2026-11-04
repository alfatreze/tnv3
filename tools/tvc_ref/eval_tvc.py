import subprocess, pathlib, json, re, sys, os, time, hashlib, threading
from concurrent.futures import ThreadPoolExecutor
S = pathlib.Path(__file__).resolve().parent
CL = pathlib.Path("<tau-v-checkout>/work/codec_phase0/ref")
OUT = pathlib.Path("<scratch>/tvceval"); OUT.mkdir(parents=True, exist_ok=True)
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]
CFG = {"tvc_v1": ["-a", "0"], "tvc_v1_aq": ["-a", "1.0"],
       "tvc_v2": ["-b", "2", "-t", "0"], "tvc_v2_t1": ["-b", "2", "-t", "1.0"], "tvc_v2_t2": ["-b", "2", "-t", "2.0"],
       "tvc_v3": ["-b", "2", "-t", "2.0", "-p", "1"]}
ACTIVE = ["tvc_v5"]  # v1 results are kept, not re-run
NF = 1440 if "long" in sys.argv[1:] else 240
LONG = pathlib.Path("<scratch>/long")
LONGCLIPS = ["long_tears", "long_anime", "long_bw"]
LONGCFG = {"long_v3": ["-s", "0"], "long_v3s": ["-s", "0.7"], "long_v4": ["-s", "0.7", "-x", "1"], "long_v5": []}
LONGQ = (26, 30, 34)
IS_LONG = "long" in sys.argv[1:]
RES = S / ("tvc_long_results.json" if IS_LONG else "tvc_v1_results.json")
if IS_LONG:
    CFG = dict(LONGCFG)
else:
    CFG.update({"tvc_v3s": ["-s", "0.7"], "tvc_v4": ["-s", "0.7", "-x", "1"], "tvc_v5": []})

def refpath(clip):
    return LONG / f"{clip}.y4m" if clip.startswith("long_") else CL / f"{clip}_800.y4m"

LOCK = threading.Lock()


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def md5(p):
    h = hashlib.md5()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def nframes(p):
    with open(p, "rb") as f:
        hdr = f.readline().decode()
    w = int(re.search(r" W(\d+)", hdr).group(1))
    h = int(re.search(r" H(\d+)", hdr).group(1))
    return (os.path.getsize(p) - len(hdr)) / (6 + w * h * 3 // 2)


def score(d, clip):
    r = run(["ffmpeg", "-hide_banner", "-i", d, "-i", str(refpath(clip)), "-lavfi",
             "[0:v]split=3[a][b][s];[1:v]split=3[c][e][t];[a][c]libvmaf=n_threads=2;[b][e]psnr;[s][t]ssim", "-f", "null", "-"])
    t = r.stderr
    v = re.search(r"VMAF score: ([0-9.]+)", t)
    p = re.search(r"PSNR y:([0-9.]+)", t)
    m = re.search(r"SSIM Y:\S+ \(\S+\) U:\S+ \(\S+\) V:\S+ \(\S+\) All:([0-9.]+)", t)
    return (float(v.group(1)) if v else None, float(p.group(1)) if p else None, float(m.group(1)) if m else None)


def job(a):
    codec, q, clip = a
    base = f"{codec}_{q}_{clip}"
    tvc, rec, dec = (str(OUT / f"{base}.{x}") for x in ("tvc", "rec.y4m", "dec.y4m"))
    try:
        t0 = time.time()
        e = run([str(S / "tvc_enc"), str(refpath(clip)), tvc, "-q", str(q)] + CFG[codec] + ["-r", rec])
        es = time.time() - t0
        if not os.path.exists(tvc) or not os.path.exists(rec) or e.returncode:
            return dict(codec=codec, q=q, clip=clip, err="enc " + (e.stderr or "")[-200:])
        size = os.path.getsize(tvc)
        d = run([str(S / "tvc_dec"), tvc, dec])
        if not os.path.exists(dec) or d.returncode:
            return dict(codec=codec, q=q, clip=clip, err="dec " + (d.stderr or "")[-200:])
        ok = md5(rec) == md5(dec)
        nf = nframes(dec)
        if nf != NF:
            return dict(codec=codec, q=q, clip=clip, err=f"frames {nf}", md5_ok=ok)
        v, p, ss = score(dec, clip)
        by = size - 16 - 4 * NF
        for x in (tvc, rec, dec):
            os.remove(x)
        if None in (v, p, ss):
            return dict(codec=codec, q=q, clip=clip, err="score missing", bytes=by, md5_ok=ok)
        return dict(codec=codec, q=q, clip=clip, bytes=by, kb_frame=by / NF / 1000, vmaf=v, psnr=p, ssim=ss,
                    enc_seconds=es, md5_ok=ok)
    except Exception as ex:
        return dict(codec=codec, q=q, clip=clip, err=str(ex))


def extra_qs(res, codec, clip):
    pts = [r for r in res if r["codec"] == codec and r["clip"] == clip and "err" not in r]
    if not pts:
        return []
    vs = [r["vmaf"] for r in pts]
    need = []
    ps = [r["psnr"] for r in pts]
    if min(vs) > 85 or min(ps) > 38:
        need.append(42)
    if max(vs) < 90 or max(ps) < 38:
        need.append(18)
    have = {r["q"] for r in pts}
    return [q for q in need if q not in have]


if __name__ == "__main__":
    res = json.load(open(RES)) if RES.exists() else []
    res = [r for r in res if "err" not in r]
    t0 = time.time()
    cnt = [0]

    def go(jobs):
        def w(j):
            r = job(j)
            with LOCK:
                res.append(r)
                cnt[0] += 1
                json.dump(res, open(RES, "w"))
                print(cnt[0], len(jobs), j, round(time.time() - t0), r.get("vmaf"), r.get("md5_ok"), r.get("err", "")[:80], flush=True)
        with ThreadPoolExecutor(10) as ex:
            list(ex.map(w, jobs))

    for phase in range(1 if IS_LONG else 2):
        done = {(r["codec"], r["q"], r["clip"]) for r in res if "err" not in r}
        if IS_LONG:
            jobs = [(c, q, cl) for cl in LONGCLIPS for c in LONGCFG for q in LONGQ]
        elif phase == 0:
            jobs = [(c, q, cl) for cl in CLIPS for c in ACTIVE for q in (22, 26, 30, 34, 38)]
        else:
            jobs = [(c, q, cl) for cl in CLIPS for c in ACTIVE for q in extra_qs(res, c, cl)]
        jobs = [j for j in jobs if j not in done]
        print("phase", phase, len(jobs), "jobs", flush=True)
        if jobs:
            go(jobs)
    bad = [r for r in res if "err" in r]
    mm = [r for r in res if r.get("md5_ok") is False]
    print(len(res), "runs;", len(bad), "errors;", len(mm), "md5 mismatches;", round(time.time() - t0), "s")
    for r in bad[:12] + mm[:12]:
        print(r)

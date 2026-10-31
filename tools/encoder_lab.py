#!/usr/bin/env python3
"""Encoder lab gate (V-280): did an encoder change make the TVC streams smaller at the same quality without breaking anything?
  python3 tools/encoder_lab.py build              compile tvc_enc and tvc_dec from the sources in this worktree
  python3 tools/encoder_lab.py run OUT.json       encode the corpus at three QPs, check every stream, write sizes and PSNR-Y (add --vmaf for VMAF too)
  python3 tools/encoder_lab.py compare BASE.json NEW.json    Bjontegaard rate change per clip and overall (negative = smaller at equal quality); exit 1 on a failed check
  python3 tools/encoder_lab.py gate               guard + frozen-decoder vectors + parse budget + run + compare against docs/encoder_baseline.json
Checks per stream: the encoder's own reconstruction equals the C reference decoder's output (md5), the stream decodes, and the decoder is the FROZEN one (this worktree's tvc_dec.c).
Clips: work/encoder_lab/ref/*_400.y4m (a symlink to the study clips, 240 frames each; the first N frames are used). The decoder, bitstream spec and RTL are not changed by this tool."""
import hashlib, json, os, pathlib, re, shlex, subprocess, sys, tempfile, time
import numpy as np
ROOT = pathlib.Path(__file__).resolve().parent.parent
TV = ROOT / "tools/tvc_ref"
ENCODER = pathlib.Path(os.environ.get("TVC_ENCODER_BINARY", str(TV / "tvc_enc"))).resolve()
if not ENCODER.is_relative_to(ROOT):
    raise ValueError("Experimental encoders must stay inside this worktree")
REF = ROOT / "work/encoder_lab/ref"
CORPUS = ["dialog", "action", "anime_dialog", "bw_room"]
QPS = [24, 30, 36]
NFRAMES = 96
def sh(*a, **k): return subprocess.run([str(x) for x in a], capture_output=True, text=True, **k)
def build():
    for n in ("tvc_enc", "tvc_dec"):
        flags = shlex.split(os.environ.get("TVC_ENCODER_CFLAGS", "")) if n == "tvc_enc" else []
        r = sh("cc", "-O2", *flags, "-o", TV / n, TV / f"{n}.c", "-lm")
        if r.returncode: print(r.stderr); sys.exit(1)
    bins_dec = ROOT / "work/encoder_lab/tvc_dec_bins"
    bins_dec.parent.mkdir(parents=True, exist_ok=True)
    r = sh("cc", "-O2", "-o", bins_dec, TV / "enc/bin_count.c", "-lm")
    if r.returncode: print(r.stderr); sys.exit(1)
    print("built tvc_enc, tvc_dec, instrumented frozen tvc_dec (bin counter)")
def y_planes(path, n):                      # luma planes of the first n frames of a y4m (the clips and the decoder output differ in colour-range tags, which makes ffmpeg's psnr filter rescale: so PSNR is computed here)
    d = pathlib.Path(path).read_bytes(); hd = d[:d.index(b"\n")].decode(); i = len(hd) + 1
    w, h = int(re.search(r" W(\d+)", hd).group(1)), int(re.search(r" H(\d+)", hd).group(1)); out = []
    for _ in range(n):
        j = d.index(b"\n", i); i = j + 1                       # "FRAME...\n"
        out.append(np.frombuffer(d, np.uint8, w * h, i).reshape(h, w).astype(np.float64)); i += w * h * 3 // 2
    return out
def quality_y(a, b, n):
    """Per-frame luma metrics on raw samples, without colour-range conversion.

    Regions match the Pocket investigation: first 100 and last 40 rows.
    The ratio/MSE threshold is diagnostic, not a universal quality standard.
    """
    pa, pb = y_planes(a, n), y_planes(b, n); v = []; spatial = []
    for frame, (x, y) in enumerate(zip(pa, pb)):
        if x.shape != y.shape or x.shape[0] < 140:
            raise ValueError('spatial metric requires matching frames at least 140 pixels high')
        e = (x - y) ** 2
        m = np.mean(e); v.append(99.0 if m == 0 else 10 * np.log10(255.0 ** 2 / m))
        top = float(np.mean(e[:100])); bottom = float(np.mean(e[-40:]))
        spatial.append(dict(frame=frame, top_mse=top, bottom_mse=bottom,
                            flagged=bottom > 80.0 and bottom > 2.0 * top))
    return float(np.mean(v)), spatial

def psnr_y(a, b, n):
    return quality_y(a, b, n)[0]
def one(clip, qp, vmaf):
    src = REF / f"{clip}_400.y4m"
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td); tvc, rec, dec = td / "a.tvc", td / "r.y4m", td / "d.y4m"
        extra = shlex.split(os.environ.get("TVC_ENCODER_ARGS", ""))
        t0 = time.time(); e = sh(ENCODER, src, tvc, "-q", qp, "-n", NFRAMES, "-r", rec, *extra); t1 = time.time()
        if e.returncode: return dict(clip=clip, qp=qp, ok=False, why="encoder failed: " + e.stderr[-200:])
        if os.environ.get("TVC_CHECK_NO_RECON") == "1":
            bare = td / "no-recon.tvc"
            check = sh(ENCODER, src, bare, "-q", qp, "-n", NFRAMES, *extra)
            if check.returncode or hashlib.sha256(tvc.read_bytes()).digest() != hashlib.sha256(bare.read_bytes()).digest():
                return dict(clip=clip, qp=qp, ok=False, why="encoding without -r failed or changed the stream")
        d = sh(ROOT / "work/encoder_lab/tvc_dec_bins", tvc, dec)
        if d.returncode: return dict(clip=clip, qp=qp, ok=False, why=f"decoder refused the stream (status {d.returncode})")
        match = re.search(r"TVC_SYNTAX_BINS=(\d+)", d.stderr)
        if not match: return dict(clip=clip, qp=qp, ok=False, why="instrumented decoder did not report syntax-bin count")
        bins = int(match.group(1))
        same = hashlib.md5(rec.read_bytes()).hexdigest() == hashlib.md5(dec.read_bytes()).hexdigest()
        import struct
        n = struct.unpack("<I", tvc.read_bytes()[12:16])[0]
        ps, spatial = quality_y(dec, src, n)
        byte_count = tvc.stat().st_size - 16 - 4 * n
        with src.open("rb") as source:
            header = source.readline().decode()
        width = int(re.search(r" W(\d+)", header).group(1))
        height = int(re.search(r" H(\d+)", header).group(1))
        mb_count = n * (((width + 15) & ~15) // 16) * (((height + 15) & ~15) // 16)
        r = dict(clip=clip, qp=qp, ok=bool(same), why="" if same else "encoder reconstruction differs from the decoder output", frames=n,
                 bytes=byte_count, psnr_y=round(ps, 3), spatial_luma=spatial, bins=bins, bins_per_byte=round(bins / byte_count, 4), bins_per_mb=round(bins / mb_count, 2), enc_s=round(t1 - t0, 1))
        if vmaf:                            # both inputs forced to the same range tag, else ffmpeg rescales one of them
            p = sh("ffmpeg", "-hide_banner", "-color_range", "tv", "-i", dec, "-color_range", "tv", "-i", src, "-lavfi", "libvmaf=n_threads=4", "-f", "null", "-")
            m = re.search(r"VMAF score: ([0-9.]+)", p.stderr); r["vmaf"] = float(m.group(1)) if m else None
        return r
def run(out, vmaf):
    res = []
    for c in CORPUS:
        for q in QPS:
            r = one(c, q, vmaf); res.append(r); print(c, q, r.get("bytes"), r.get("psnr_y"), "bins/B", r.get("bins_per_byte"), "bins/MB", r.get("bins_per_mb"), "spatial flags", sum(x['flagged'] for x in r.get('spatial_luma', [])), "OK" if r["ok"] else "FAIL " + r["why"], flush=True)
    pathlib.Path(out).parent.mkdir(parents=True, exist_ok=True)
    inputs = {c: hashlib.sha256((REF / f"{c}_400.y4m").read_bytes()).hexdigest() for c in CORPUS}
    pathlib.Path(out).write_text(json.dumps(dict(nframes=NFRAMES, qps=QPS, corpus=CORPUS, source_sha256=inputs, spatial_metric="raw luma MSE, first 100 and last 40 rows; flag if bottom > 80 and bottom > 2 * top; zero-based frames; diagnostic only", encoder_binary=str(ENCODER), encoder_sha256=hashlib.sha256(ENCODER.read_bytes()).hexdigest(), external_encoder=ENCODER != TV / "tvc_enc", encoder_args=shlex.split(os.environ.get("TVC_ENCODER_ARGS", "")), encoder_cflags=shlex.split(os.environ.get("TVC_ENCODER_CFLAGS", "")), no_recon_checked=os.environ.get("TVC_CHECK_NO_RECON") == "1", results=res), indent=1))
    return all(r["ok"] for r in res)
def bd_rate(a, b):                          # a, b: lists of (bytes, psnr); Bjontegaard rate change of b against a, in percent
    la, lb = np.log([x[0] for x in a]), np.log([x[0] for x in b]); pa, pb = [x[1] for x in a], [x[1] for x in b]
    lo, hi = max(min(pa), min(pb)), min(max(pa), max(pb))
    if hi <= lo: return float("nan")
    fa, fb = np.polyfit(pa, la, 2), np.polyfit(pb, lb, 2)
    ia, ib = np.polyint(fa), np.polyint(fb)
    avg = ((np.polyval(ib, hi) - np.polyval(ib, lo)) - (np.polyval(ia, hi) - np.polyval(ia, lo))) / (hi - lo)
    return (np.exp(avg) - 1) * 100
def compare(base, new):
    B, N = json.loads(pathlib.Path(base).read_text()), json.loads(pathlib.Path(new).read_text())
    ok = all(r["ok"] for r in N["results"]); tot = []
    ok &= N["corpus"] == B["corpus"] == CORPUS and N["qps"] == B["qps"] == QPS
    ok &= len(N["results"]) == len(CORPUS) * len(QPS)
    ok &= sorted((r["clip"], r["qp"]) for r in N["results"]) == sorted((c, q) for c in CORPUS for q in QPS)
    for c in N["corpus"]:
        ra = [(r["bytes"], r["psnr_y"]) for r in B["results"] if r["clip"] == c and r["ok"]]; rb = [(r["bytes"], r["psnr_y"]) for r in N["results"] if r["clip"] == c and r["ok"]]
        d = bd_rate(ra, rb) if len(ra) == len(rb) == len(QPS) else float("nan"); tot.append(d)
        ok &= bool(np.isfinite(d) and d <= 1.0)
        ca = [(r["bins"], r["psnr_y"]) for r in B["results"] if r["clip"] == c and r.get("bins") and r["ok"]]
        cn = [(r["bins"], r["psnr_y"]) for r in N["results"] if r["clip"] == c and r.get("bins") and r["ok"]]
        cost = bd_rate(ca, cn) if len(ca) == len(cn) == len(QPS) else float("nan")
        ok &= bool(np.isfinite(cost) and cost <= 1e-9)
        ba = [r.get("bins_per_byte") for r in B["results"] if r["clip"] == c]; bn = [r.get("bins_per_byte") for r in N["results"] if r["clip"] == c]
        ma = [r.get("bins_per_mb") for r in B["results"] if r["clip"] == c]; mn = [r.get("bins_per_mb") for r in N["results"] if r["clip"] == c]
        btxt = (f"bins/B {np.nanmean(bn):.3f} vs {np.nanmean(ba):.3f}; bins/MB {np.nanmean(mn):.2f} vs {np.nanmean(ma):.2f}"
                if all(x is not None for x in ba + bn + ma + mn) else "bin baseline pending")
        print(f"{c:14s} BD-rate {d:+6.2f} %   equal-quality bin change {cost:+6.2f} %   {btxt}")
    ba = [r.get("bins_per_byte") for r in B["results"]]; bn = [r.get("bins_per_byte") for r in N["results"]]
    ma = [r.get("bins_per_mb") for r in B["results"]]; mn = [r.get("bins_per_mb") for r in N["results"]]
    btxt = (f"mean bins/B {np.nanmean(bn):.3f} vs {np.nanmean(ba):.3f}; bins/MB {np.nanmean(mn):.2f} vs {np.nanmean(ma):.2f}"
            if all(x is not None for x in ba + bn + ma + mn) else "mean bin baseline pending")
    ok &= bool(np.all(np.isfinite(tot)) and np.mean(tot) < 0)
    # Compare parser work at equal quality above; also require matching frame counts.
    for r in N["results"]:
        prior = [x for x in B["results"] if (x["clip"], x["qp"]) == (r["clip"], r["qp"])]
        if len(prior) != 1 or "bins" not in r or "bins" not in prior[0] or r["frames"] != prior[0]["frames"]:
            print("parser work FAILED:", r["clip"], r["qp"])
            ok = False
    print(f"{'mean':14s} BD-rate {np.nanmean(tot):+6.2f} %   {btxt}   checks {'all passed' if ok else 'FAILED'}")
    return ok
def gate():
    ok = sh(sys.executable, ROOT / "tools/encoder_guard.py").returncode == 0; print("guard", "ok" if ok else "FAILED")
    build()
    v = sh(sys.executable, TV / "check_c_vectors.py"); ok &= v.returncode == 0; print("frozen decoder vectors:", v.stdout.strip().splitlines()[-1] if v.stdout.strip() else v.stderr[-100:])
    ok &= sh(sys.executable, TV / "parse_budget.py").returncode == 0
    out = pathlib.Path(os.environ.get("TVC_GATE_OUTPUT", str(ROOT / "work/encoder_lab" / f"gate-{ENCODER.name}.json"))); ok &= run(out, False)
    base = ROOT / "docs/encoder_baseline.json"
    if base.exists(): ok &= compare(base, out)
    print("GATE", "PASSED" if ok else "FAILED"); return ok
if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["build"]: build()
    elif a[:1] == ["run"]: sys.exit(0 if run(a[1], "--vmaf" in a) else 1)
    elif a[:1] == ["compare"]: sys.exit(0 if compare(a[1], a[2]) else 1)
    elif a[:1] == ["gate"]: sys.exit(0 if gate() else 1)
    else: print(__doc__)

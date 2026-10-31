#!/usr/bin/env python3
"""Phase-0 rate study: bitrate vs VMAF/SSIM/PSNR for each encoder configuration.

Every encode is decoded, scaled (400-wide encodes) back to 800x334 with Lanczos
and compared against the 800x334 reference, so the resolution-switch question
is answered on the same scale as native coding.  Results are appended to
work/codec_phase0/results.jsonl (resumable: finished jobs are skipped).
"""
import concurrent.futures as cf, json, pathlib, subprocess, sys, threading, time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from clips import CLIPS as CLIPDEF

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
REF = ROOT / "ref"
ENC = ROOT / "enc"
RES = ROOT / "results.jsonl"
CLIPS = list(CLIPDEF)
FPS, SECS = 24, 10

X264_Q = [18, 22, 26, 30, 34, 38, 42]
X265_Q = [20, 24, 28, 32, 36, 40, 44]
SVT_Q = [26, 32, 38, 44, 50, 56, 62]
M4_Q = [2, 4, 6, 9, 13, 19, 27]


def x264(profile, extra=(), params=""):
    def f(q):
        a = ["-c:v", "libx264", "-preset", "slow", "-profile:v", profile, "-crf", str(q), "-g", "96",
             "-threads", "2", *extra]
        if params:
            a += ["-x264-params", params]
        return a + ["-f", "h264"], "h264"
    return f


def svt(params="", preset="6"):
    def f(q):
        return ["-c:v", "libsvtav1", "-preset", preset, "-crf", str(q), "-g", "96",
                "-svtav1-params", "lp=2" + (":" + params if params else ""), "-f", "ivf"], "ivf"
    return f


def m4(tuned):
    def f(q):
        a = ["-c:v", "mpeg4", "-qscale:v", str(q), "-g", "96", "-bf", "0", "-threads", "2"]
        if tuned:
            a += ["-mbd", "rd", "-trellis", "1", "-cmp", "2", "-subcmp", "2", "-flags", "+aic"]
        return a + ["-f", "m4v"], "m4v"
    return f


def x265(q):
    return ["-c:v", "libx265", "-preset", "medium", "-crf", str(q), "-g", "96", "-x265-params",
            "log-level=error:pools=2", "-f", "hevc"], "hevc"


def aom(flags="", cpu="4"):
    """libaom (aomenc) in constrained-quality mode; `flags` is a space-separated list of aomenc options."""
    def f(q, src, out):
        return ["aomenc", str(src), "-o", str(out), "--quiet", "--end-usage=q", f"--cq-level={q}", f"--cpu-used={cpu}",
                "--threads=2", "--kf-max-dist=96", "--bit-depth=8", "--limit=240", "--passes=2",
                *(["--lag-in-frames=19"] if "--lag-in-frames" not in flags else []), *flags.split()], "ivf"
    f.native = True
    return f


# name: (builder, quality list, resolutions)
CONFIGS = {
    "mpeg4_base":        (m4(False), M4_Q, ["800"]),
    "mpeg4_tuned":       (m4(True), M4_Q, ["800", "400"]),
    "x264_cavlc_nob":    (x264("baseline"), X264_Q, ["800"]),
    "x264_main_nob":     (x264("main", ["-bf", "0", "-refs", "3"]), X264_Q, ["800"]),
    "x264_main_b":       (x264("main", ["-bf", "3", "-refs", "3"]), X264_Q, ["800", "400"]),
    "x264_high":         (x264("high"), X264_Q, ["800"]),
    "x264_main_b_mvr16": (x264("main", ["-bf", "3", "-refs", "3"], "mvrange=16"), X264_Q, ["800"]),
    "x264_main_b_nodbk": (x264("main", ["-bf", "3", "-refs", "3"], "no-deblock=1"), X264_Q, ["800"]),
    "x264_main_b_cavlc": (x264("main", ["-bf", "3", "-refs", "3", "-coder", "0"]), X264_Q, ["800"]),
    "x264_main_b_ref1":  (x264("main", ["-bf", "3", "-refs", "1"]), X264_Q, ["800"]),
    "x265_medium":       (x265, X265_Q, ["800", "400"]),
    "svt_full":          (svt(), SVT_Q, ["800", "400"]),
    "svt_nocdef_norest": (svt("enable-cdef=0:enable-restoration=0"), SVT_Q, ["800"]),
    "svt_lowdelay":      (svt("pred-struct=1"), SVT_Q, ["800"]),
    # AV1 decomposition: which parts of the gain are structure (B depth, temporal filter) and which are tools
    "svt_notf":          (svt("enable-tf=0:enable-kf-tf=0"), SVT_Q, ["800"]),
    "svt_hier2":         (svt("hierarchical-levels=2"), SVT_Q, ["800"]),
    "svt_hier3":         (svt("hierarchical-levels=3"), SVT_Q, ["800"]),
    "svt_tx32":          (svt("max-tx-size=32"), SVT_Q, ["800"]),
    "svt_noloopf":       (svt("enable-dlf=0:enable-cdef=0:enable-restoration=0"), SVT_Q, ["800"]),
    "svt_hier3_tx32_nocdef": (svt("hierarchical-levels=3:max-tx-size=32:enable-cdef=0:enable-restoration=0"), SVT_Q, ["800"]),
    "svt_p4":            (svt("", "4"), SVT_Q, ["800"]),
    "svt_p9":            (svt("", "9"), SVT_Q, ["800"]),
    "svt_fg":            (svt("film-grain=6:film-grain-denoise=1"), SVT_Q, ["800"]),
}

# libaom: every tool that has an encoder switch, one at a time against the full-tool baseline
AOM_Q = [32, 38, 44, 50, 54, 58, 62]
AOM = {
    "aom_full": "",
    "aom_nocdef": "--enable-cdef=0",
    "aom_norest": "--enable-restoration=0",
    "aom_notx64": "--enable-tx64=0",
    "aom_noobmc": "--enable-obmc=0",
    "aom_nowarp": "--enable-warped-motion=0",
    "aom_noglobal": "--enable-global-motion=0",
    "aom_nopalette": "--enable-palette=0",
    "aom_nointrabc": "--enable-intrabc=0",
    "aom_nofilterintra": "--enable-filter-intra=0",
    "aom_nocfl": "--enable-cfl-intra=0",
    "aom_nosmoothpaeth": "--enable-smooth-intra=0 --enable-paeth-intra=0",
    "aom_nointraedge": "--enable-intra-edge-filter=0",
    "aom_noangledelta": "--enable-angle-delta=0",
    "aom_nointerintra": "--enable-interintra-comp=0",
    "aom_nomasked": "--enable-masked-comp=0",
    "aom_nodistwtd": "--enable-dist-wtd-comp=0",
    "aom_nodualfilter": "--enable-dual-filter=0",
    "aom_norect": "--enable-rect-partitions=0",
    "aom_noab": "--enable-ab-partitions=0",
    "aom_no1to4": "--enable-1to4-partitions=0",
    "aom_maxpart32": "--max-partition-size=32",
    "aom_maxpart16": "--max-partition-size=16",
    "aom_sb64": "--sb-size=64",
    "aom_noreffmvs": "--enable-ref-frame-mvs=0",
    "aom_norecttx": "--enable-rect-tx=0",
    "aom_noflipidtx": "--enable-flip-idtx=0",
    "aom_reducedtx": "--reduced-tx-type-set=1",
    "aom_nocdfupdate": "--cdf-update-mode=0",
    "aom_pyr3": "--gf-max-pyr-height=3",
    "aom_pyr2": "--gf-max-pyr-height=2",
    "aom_refs2": "--max-reference-frames=2",
    "aom_refs3": "--max-reference-frames=3",
    "aom_lowdelay": "--lag-in-frames=0",
    "aom_noarnr": "--arnr-maxframes=0",
    "aom_notpl": "--enable-tpl-model=0",
    "aom_nodeltaq": "--deltaq-mode=0",
    "aom_notrellis": "--disable-trellis-quant=1",
    "aom_p3": ("", "3"),
    "aom_p6": ("", "6"),
}
# Combinations built from the single-tool results: every tool that cost nothing alone, removed together,
# then the same plus the cheaper structural simplifications.
LEAN_ZERO = " ".join([
    "--enable-cdef=0", "--enable-obmc=0", "--enable-palette=0", "--enable-intrabc=0", "--enable-filter-intra=0",
    "--enable-cfl-intra=0", "--enable-smooth-intra=0", "--enable-paeth-intra=0", "--enable-intra-edge-filter=0",
    "--enable-interintra-comp=0", "--enable-smooth-interintra=0", "--enable-interintra-wedge=0",
    "--enable-masked-comp=0", "--enable-interinter-wedge=0", "--enable-diff-wtd-comp=0", "--enable-dist-wtd-comp=0",
    "--enable-dual-filter=0", "--enable-flip-idtx=0", "--enable-tx64=0", "--enable-warped-motion=0",
    "--enable-global-motion=0", "--enable-angle-delta=0", "--enable-ref-frame-mvs=0", "--enable-1to4-partitions=0",
    "--enable-ab-partitions=0"])
# AV1 film-grain synthesis: libaom estimates the noise, removes it before coding and sends a parametric model.
# VMAF/SSIM/PSNR score the output against the GRAINY reference, so they punish the removed grain: read the size
# saving together with the quality drop, and judge grain by eye.
AOM["aom_fg10"] = "--denoise-noise-level=10"
AOM["aom_fg25"] = "--denoise-noise-level=25"
AOM["aom_lean0"] = LEAN_ZERO
AOM["aom_lean1"] = LEAN_ZERO + " --enable-restoration=0 --max-partition-size=32 --gf-max-pyr-height=3 --max-reference-frames=3"
AOM["aom_lean2"] = LEAN_ZERO + " --enable-restoration=0 --enable-rect-partitions=0 --max-partition-size=32 --gf-max-pyr-height=3 --max-reference-frames=3"
for _n, _f in AOM.items():
    _flags, _cpu = _f if isinstance(_f, tuple) else (_f, "4")
    CONFIGS[_n] = (aom(_flags, _cpu), AOM_Q, ["800", "400"] if _n == "aom_full" else ["800"])

lock = threading.Lock()


def run_job(job):
    clip, cfg, res, q = job
    build, _, _ = CONFIGS[cfg]
    ref_in = REF / f"{clip}_{res}.y4m"
    if getattr(build, "native", False):
        ext = build(q, ref_in, "x")[1]
        out = ENC / f"{clip}_{cfg}_{res}_{q}.{ext}"
        cmd = build(q, ref_in, out)[0]
    else:
        args, ext = build(q)
        out = ENC / f"{clip}_{cfg}_{res}_{q}.{ext}"
        cmd = ["ffmpeg", "-v", "error", "-y", "-i", str(ref_in), "-an", *args, str(out)]
    tag = f"{clip}_{cfg}_{res}_{q}"
    t0 = time.time()
    p = subprocess.run(cmd, capture_output=True, text=True)
    if p.returncode or not out.exists():
        return {"clip": clip, "cfg": cfg, "res": res, "q": q, "error": "enc: " + p.stderr[-300:]}
    t_enc = time.time() - t0
    bits = out.stat().st_size * 8
    log = ENC / f"{tag}.json"
    cw, ch = CLIPDEF[clip]["w"], CLIPDEF[clip]["h"]
    lav = (f"[0:v]scale={cw}:{ch}:flags=lanczos,setpts=N/({FPS}*TB)[d];[1:v]setpts=N/({FPS}*TB)[r];"
           f"[d][r]libvmaf=model=version=vmaf_v0.6.1:feature=name=psnr|name=float_ssim:log_fmt=json:"
           f"log_path={log}:n_threads=2")
    rate = [] if ext == "ivf" else ["-framerate", str(FPS)]
    p = subprocess.run(["ffmpeg", "-v", "error", *rate, "-i", str(out), "-i",
                        str(REF / f"{clip}_800.y4m"), "-lavfi", lav, "-f", "null", "-"], capture_output=True, text=True)
    if p.returncode or not log.exists():
        return {"clip": clip, "cfg": cfg, "res": res, "q": q, "error": "vmaf: " + p.stderr[-300:]}
    m = json.load(open(log))["pooled_metrics"]
    rec = {"clip": clip, "cfg": cfg, "res": res, "q": q, "kbps": bits / SECS / 1000,
           "vmaf": m["vmaf"]["mean"], "vmaf_min": m["vmaf"]["min"], "ssim": m["float_ssim"]["mean"],
           "psnr_y": m["psnr_y"]["mean"], "enc_s": t_enc, "bytes": bits // 8}
    out.unlink()           # keep the table, drop the streams
    log.unlink()
    return rec


def main():
    ENC.mkdir(parents=True, exist_ok=True)
    done = set()
    if RES.exists():
        for line in RES.read_text().splitlines():
            r = json.loads(line)
            if "error" not in r:
                done.add((r["clip"], r["cfg"], r["res"], r["q"]))
    args = sys.argv[1:]
    clips = CLIPS
    if "--clips" in args:
        i = args.index("--clips")
        clips = args[i + 1].split(",")
        del args[i:i + 2]
    par = int(args[0]) if args else 6
    only = set(args[1:]) or None
    jobs = [(c, n, r, q) for c in clips for n, (_, qs, rs) in CONFIGS.items() for r in rs for q in qs
            if (c, n, r, q) not in done and (not only or n in only)]
    print(len(jobs), "jobs", flush=True)
    with cf.ThreadPoolExecutor(par) as ex, open(RES, "a") as fh:
        for i, rec in enumerate(ex.map(run_job, jobs), 1):
            with lock:
                fh.write(json.dumps(rec) + "\n")
                fh.flush()
            print(i, len(jobs), rec["clip"], rec["cfg"], rec["res"], rec["q"],
                  rec.get("error") or f'{rec["kbps"]:.0f}kbps vmaf {rec["vmaf"]:.1f}', flush=True)


if __name__ == "__main__":
    main()

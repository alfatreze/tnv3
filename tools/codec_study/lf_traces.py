#!/usr/bin/env python3
"""Loop filter traces (AV1_LF_TRACE) of the study streams and of a few streams that exercise filter parameters the study set does not
(sharpness, delta loop filter, other quantisers) for sim/av1/lf_check.c (V-084).
  python3 tools/codec_study/lf_traces.py [TUS]      -> build/lf_NAME.bin
"""
import os, pathlib, shlex, struct, subprocess, sys
ROOT = pathlib.Path(__file__).resolve().parents[2]
n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
s = (ROOT / "tools/codec_study/sweep.py").read_text()
g = {"AOM": {}}
exec(s[s.index("LEAN_ZERO = "):s.index('AOM["aom_lean2"]')], g)
lean2 = g["LEAN_ZERO"] + " --enable-restoration=0 --enable-rect-partitions=0 --max-partition-size=32 --gf-max-pyr-height=3 --max-reference-frames=3"
extra = ROOT / "work/codec_phase0/av1lf"
extra.mkdir(parents=True, exist_ok=True)
y4m = ROOT / "build" / "mt_in.y4m"
if y4m.exists():
    for name, opts in (("sharp3", "--sharpness=3 --cq-level=36"), ("sharp7", "--sharpness=7 --cq-level=44"), ("dlf", "--deltaq-mode=1 --delta-lf-mode=1 --cq-level=32"),
                       ("lowq", "--cq-level=6"), ("hiq", "--cq-level=58")):
        ivf = extra / f"{name}_lean2.ivf"
        if not ivf.exists():
            subprocess.run(f"aomenc {shlex.quote(str(y4m))} --limit=6 --cpu-used=4 --end-usage=q --min-q=0 --max-q=63 --lag-in-frames=8 {lean2} {opts} -o {shlex.quote(str(ivf))}",
                           shell=True, check=True, capture_output=True, input=b"y\n")
for d in ("av1", "av1mt", "av1lf"):
    for ivf in sorted((ROOT / "work/codec_phase0" / d).glob("*_lean2.ivf")):
        src = ivf.read_bytes()
        hl = struct.unpack("<H", src[6:8])[0]
        pos, tus = hl, []
        while pos < len(src):
            sz = struct.unpack("<I", src[pos:pos + 4])[0]
            tus.append(src[pos:pos + 12 + sz]); pos += 12 + sz
        hdr = bytearray(src[:hl]); struct.pack_into("<I", hdr, 24, min(n, len(tus)))
        tmp = ROOT / "build" / f"trunc_{ivf.name}"
        tmp.write_bytes(bytes(hdr) + b"".join(tus[:n]))
        out = ROOT / "build" / f"lf_{ivf.name[:-len('_lean2.ivf')]}.bin"
        subprocess.run([str(ROOT / "build/av1trace"), str(tmp)], env=dict(os.environ, AV1_LF_TRACE=str(out)), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        tmp.unlink()
        print(out.name, (out.stat().st_size // 1_000_000) if out.exists() else "no filtered frame", flush=True)

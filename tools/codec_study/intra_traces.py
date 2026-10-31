#!/usr/bin/env python3
"""Intra prediction traces of the study streams for sim/av1_intra_model.py and sim/av1_intra_edges.py (V-083).
  python3 tools/codec_study/intra_traces.py [TUS]     first TUS temporal units (default 12) of every study stream -> build/intra_NAME.txt
"""
import os, pathlib, struct, subprocess, sys
ROOT = pathlib.Path(__file__).resolve().parents[2]
n = int(sys.argv[1]) if len(sys.argv) > 1 else 12
for d in ("av1", "av1mt"):
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
        out = ROOT / "build" / f"intra_{ivf.name[:-len('_lean2.ivf')]}.txt"
        subprocess.run([str(ROOT / "build/av1trace"), str(tmp)], env=dict(os.environ, AV1_INTRA_TRACE=str(out)),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        tmp.unlink()
        print(out.name, out.stat().st_size // 1_000_000, "MB", flush=True)

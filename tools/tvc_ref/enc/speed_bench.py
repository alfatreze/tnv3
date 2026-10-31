#!/usr/bin/env python3
"""Alternate two encoder binaries; measure median speed and optionally require identical streams.

Run with no other encodes/builds active. Example:
  python3 tools/tvc_ref/enc/speed_bench.py work/encoder_lab/encoder-pre-speed tools/tvc_ref/tvc_enc --exact --out work/encoder_lab/speed.json
"""
import argparse
import hashlib
import json
import pathlib
import platform
import shlex
import statistics
import struct
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("before", type=pathlib.Path)
    ap.add_argument("after", type=pathlib.Path)
    ap.add_argument("--out", type=pathlib.Path, required=True)
    ap.add_argument("--frames", type=int, default=96)
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--suffix", choices=["400", "800"], default="400")
    ap.add_argument("--cases", nargs="+", default=["action:30", "anime_dialog:24", "bw_room:36"])
    ap.add_argument("--exact", action="store_true")
    ap.add_argument("--encoder-args", default="", help="additional options passed to both encoders")
    a = ap.parse_args()
    if a.frames <= 0 or a.repeats <= 0:
        ap.error("frames and repeats must be positive")
    binaries = {name: getattr(a, name).resolve() for name in ("before", "after")}
    extra = shlex.split(a.encoder_args)
    report = dict(platform=platform.platform(), machine=platform.machine(), frames=a.frames, encoder_args=extra,
                  repeats=a.repeats, suffix=a.suffix, binaries={k:dict(path=str(p), sha256=digest(p)) for k,p in binaries.items()}, samples=[], summary=[])
    with tempfile.TemporaryDirectory() as td:
        for rep in range(a.repeats):
            for case in a.cases:
                clip, qp = case.split(":")
                src = ROOT / "work/encoder_lab/ref" / f"{clip}_{a.suffix}.y4m"
                for name in (["before", "after"] if rep % 2 == 0 else ["after", "before"]):
                    dst = pathlib.Path(td) / "out.tvc"
                    start = time.perf_counter()
                    process = subprocess.run([str(binaries[name]), str(src), str(dst), "-q", qp, "-n", str(a.frames), *extra], check=True, capture_output=True)
                    elapsed = time.perf_counter() - start
                    with dst.open("rb") as stream:
                        header = stream.read(16)
                    if len(header) != 16 or struct.unpack_from("<I", header, 12)[0] != a.frames:
                        raise RuntimeError("benchmark did not encode the requested frame count")
                    r = dict(case=case, version=name, repetition=rep, seconds=elapsed, stream_sha256=digest(dst), diagnostics=process.stderr.decode(errors="replace")[-2000:])
                    report["samples"].append(r)
                    print(case, name, f"{elapsed:.3f}s", flush=True)
    exact = True
    for case in a.cases:
        rows = [r for r in report["samples"] if r["case"] == case]
        same = len({r["stream_sha256"] for r in rows}) == 1
        exact &= same
        med = {name:statistics.median(r["seconds"] for r in rows if r["version"] == name) for name in binaries}
        row = dict(case=case, median_seconds=med, fps={k:a.frames/v for k,v in med.items()}, speedup=med["before"]/med["after"], identical_streams=same)
        report["summary"].append(row)
        print(json.dumps(row), flush=True)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if exact or not a.exact else 1

if __name__ == "__main__":
    raise SystemExit(main())

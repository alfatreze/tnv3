#!/usr/bin/env python3
"""Run the block-trace decoder (build/av1trace, libaom patched with sim/av1/patches/blocktrace.patch) over the AV1 streams of the
workload study.

  python3 tools/codec_study/blk_traces.py [NAME-SUBSTRING ...]     # writes work/codec_phase0/av1/<clip>_<cfg>.blk
Then:  python3 sim/av1_blk_model.py build/fc_layout.txt build/blk_tables.txt work/codec_phase0/av1/*.blk
(about 60 MB of trace per intra frame-heavy stream; the files are git-ignored scratch)
"""
import os, pathlib, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "work" / "codec_phase0" / "av1"
pats = sys.argv[1:]
for ivf in sorted(OUT.glob("*.ivf")):
    if pats and not any(p in ivf.name for p in pats):
        continue
    blk = ivf.with_suffix(".blk")
    if blk.exists():
        continue
    env = dict(os.environ, AV1_BLK_TRACE=str(blk))
    subprocess.run([str(ROOT / "build" / "av1trace"), str(ivf)], env=env, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, check=True)
    print(blk.name, blk.stat().st_size // 1_000_000, "MB", flush=True)

#!/usr/bin/env python3
"""Run the coefficient-trace decoder (build/av1trace) over every AV1 stream of the workload study.

  python3 tools/codec_study/coef_traces.py          # writes work/codec_phase0/av1/<clip>_<cfg>.coef
Then:  python3 sim/av1_coef_model.py build/coef_tables.txt work/codec_phase0/av1/*.coef
"""
import os, pathlib, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "work" / "codec_phase0" / "av1"
for ivf in sorted(OUT.glob("*.ivf")):
    coef = ivf.with_suffix(".coef")
    if coef.exists():
        continue
    env = dict(os.environ, AV1_COEF_TRACE=str(coef))
    subprocess.run([str(ROOT / "build" / "av1trace"), str(ivf)], env=env, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, check=True)
    print(coef.name, coef.stat().st_size // 1_000_000, "MB")

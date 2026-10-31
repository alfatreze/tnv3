#!/usr/bin/env python3
"""Block until a sweep log shows its final job (or the time limit passes); print progress.

usage: wait_sweep.py [seconds] [log name, default sweep.log]
"""
import pathlib, sys, time

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
limit = float(sys.argv[1]) if len(sys.argv) > 1 else 540
LOG = ROOT / (sys.argv[2] if len(sys.argv) > 2 else "sweep.log")
t0 = time.time()
last = ""
while time.time() - t0 < limit:
    lines = LOG.read_text().splitlines() if LOG.exists() else []
    last = lines[-1] if lines else ""
    parts = last.split()
    if len(parts) > 2 and parts[0].isdigit() and parts[0] == parts[1]:
        break
    time.sleep(10)
print(last)

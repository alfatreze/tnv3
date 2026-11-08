#!/usr/bin/env python3
"""Verify the pinned Tau V files (tau_v_pins.json) by SHA-256.

Root: --root PATH, else env TAU_V_ROOT, else the path recorded in the manifest
relative to this checkout's parent (../tau-nu/.claude/worktrees/codec-alternatives).
A missing or changed file is a failure (exit 1 changed, 2 missing); nothing is
skipped silently. --skip-films leaves out the large films, and says so.
"""
import argparse, hashlib, json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
ap = argparse.ArgumentParser()
ap.add_argument("--root")
ap.add_argument("--skip-films", action="store_true")
a = ap.parse_args()
m = json.load(open(os.path.join(here, "tau_v_pins.json")))
root = a.root or os.environ.get("TAU_V_ROOT") or os.path.normpath(
    os.path.join(here, "..", "..", "..", m["source_repo"]))
changed = missing = skipped = 0
for f in m["files"]:
    if a.skip_films and f["kind"] == "film":
        skipped += 1
        print("SKIPPED", f["path"])
        continue
    p = os.path.join(root, f["path"])
    if not os.path.isfile(p):
        missing += 1
        print("MISSING", p)
        continue
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for blk in iter(lambda: fh.read(1 << 20), b""):
            h.update(blk)
    if h.hexdigest() != f["sha256"]:
        changed += 1
        print("CHANGED", f["path"], h.hexdigest())
    else:
        print("ok     ", f["path"])
print(f"root={root} missing={missing} changed={changed} skipped={skipped}")
sys.exit(2 if missing else 1 if changed else 0)

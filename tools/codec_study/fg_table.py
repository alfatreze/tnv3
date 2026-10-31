#!/usr/bin/env python3
"""Film-grain synthesis: size saved and quality lost at the SAME encoder quality setting (cq 44 and 50).

The metrics compare against the grainy reference, so a lower score is expected; the saving is what matters.
"""
import json, pathlib
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
d = {}
for ln in (ROOT / "results.jsonl").read_text().splitlines():
    r = json.loads(ln)
    if "error" not in r:
        d[(r["clip"], r["cfg"], r["res"], r["q"])] = r
clips = sorted({k[0] for k in d})
print("| clip | cq | aom_full kbps / VMAF | fg10 size / VMAF | fg25 size / VMAF |")
print("|---|---|---|---|---|")
tot = defaultdict(list)
for q in (44, 50):
    for c in clips:
        b = d.get((c, "aom_full", "800", q))
        if not b:
            continue
        row = [f"{b['kbps']:.0f} / {b['vmaf']:.1f}"]
        for cfg in ("aom_fg10", "aom_fg25"):
            r = d.get((c, cfg, "800", q))
            if r:
                row.append(f"{(r['kbps'] / b['kbps'] - 1) * 100:+.0f}% / {r['vmaf'] - b['vmaf']:+.1f}")
                tot[(cfg, q, c.startswith("bw"))].append(r["kbps"] / b["kbps"])
            else:
                row.append("-")
        print(f"| {c} | {q} | " + " | ".join(row) + " |")
print()
for (cfg, q, bw), v in sorted(tot.items()):
    print(f"{cfg} cq{q} {'B&W' if bw else 'colour'}: mean size {(sum(v) / len(v) - 1) * 100:+.0f}% ({len(v)} clips)")

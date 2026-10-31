#!/usr/bin/env python3
"""One-line-per-clip summary of work/codec_phase0/workload.json."""
import json, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
for r in json.load(open(ROOT / "workload.json")):
    fb = r["frame_bytes"]
    print(r["clip"], "crf", r["crf"], "kbps %.0f" % r["kbps"], "| |dy| p50/90/99/99.9:", r["dy_p50"], r["dy_p90"],
          r["dy_p99"], r["dy_p999"], "| |dx| p99/99.9:", r["dx_p99"], r["dx_p999"],
          "| future-ref share %.2f" % r["bi_or_future_share"])
    print("   frames (n, mean B, max B):", {t: (v["n"], round(v["mean"]), v["max"]) for t, v in fb.items()},
          "| max/mean %.1f" % r["frame_max_over_mean"], "| worst second/mean %.2f" % r["worst_second_over_mean"])
    print("   x264 P-MB:", r["x264_mb"].get("mb P", ""))
    print("   x264 B-MB:", r["x264_mb"].get("mb B", ""))
    print("   blocks:", r["block_mix_top"])

#!/usr/bin/env python3
"""Path guard for the encoder lab (V-280): the encoder agent may change the encoder and its tools, nothing the decoder, the RTL or the firmware depend on.
  python3 tools/encoder_guard.py [BASE]     (BASE defaults to the tag tvc-dec-draft5: the frozen decoder side)
Lists every file that differs from BASE (committed, staged or in the working tree) outside the allowed paths; exit 1 if there is any.
A change that needs a frozen path (a bitstream syntax change, tvc.h, the decoder) is NOT an encoder change: it needs a new draft of docs/TVC_BITSTREAM_V0.md, the owner's decision
and decoder / RTL work, in another worktree."""
import subprocess, sys
BASE = sys.argv[1] if len(sys.argv) > 1 else "tvc-dec-draft5"
ALLOWED = ("tools/tvc_ref/tvc_enc.c", "tools/tvc_ref/enc/", "tools/tvc_ref/quick_try.py", "tools/tvc_ref/eval_tvc.py", "tools/tvc_ref/eval_an.py", "tools/tvc_ref/eval_long_an.py",
           "tools/tvc_ref/make_long_clips.py", "tools/tvc_ref/tvc_v1_results.json", "tools/tvc_ref/tvc_long_results.json", "tools/tnv3_ref/encode_film.py",
           "tools/tnv3_ref/film_sweep.py", "tools/encoder_lab.py", "tools/encoder_guard.py", "docs/ENCODER_", "docs/encoder_", "work/")
def lines(*a): return [l for l in subprocess.run(["git", *a], capture_output=True, text=True, check=True).stdout.splitlines() if l]
changed = set(lines("diff", "--name-only", BASE)) | set(lines("ls-files", "--others", "--exclude-standard"))
bad = sorted(f for f in changed if not f.startswith(ALLOWED))
for f in bad: print("FROZEN:", f)
print(f"encoder_guard: {len(changed)} changed file(s) against {BASE}, {len(bad)} outside the allowed paths")
sys.exit(1 if bad else 0)

#!/usr/bin/env python3
"""Coarse POSIX CPU phase profile from an instrumented temporary encoder copy.
Build the production encoder with -O3 first; --verify compares every stream.
No production source is edited. Timings include source reading and output I/O.
"""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[3]

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=pathlib.Path, required=True)
    ap.add_argument("--verify", action="store_true")
    a = ap.parse_args()
    work = ROOT / "work/encoder_lab"
    work.mkdir(parents=True, exist_ok=True)
    encoder_source = ROOT / "tools/tvc_ref/tvc_enc.c"
    source_sha256 = hashlib.sha256(encoder_source.read_bytes()).hexdigest()
    deps = [ROOT / "tools/tvc_ref/tvc.h", *(ROOT / "tools/tvc_ref/enc" / name for name in ("pixel_ops.h", "bit_ops.h", "qpel.h", "rdoq.h"))]
    dependency_sha256 = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in deps}
    src = encoder_source.read_text()
    src = '#include <time.h>\nstatic double prof_now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}\n' + src
    replacements = [
        ('int main(int argc, char **argv) {', 'int main(int argc, char **argv) {\n double prof_start=prof_now(), prof_look_start=0,prof_look_end=0,prof_encode_start=0,prof_encode_end=0;'),
        ('Look look = look_costs(src, n);', 'prof_look_start=prof_now(); Look look = look_costs(src, n); prof_look_end=prof_now();'),
        ('size_t total = 0; RC r;', 'prof_encode_start=prof_now(); size_t total = 0; RC r;'),
        ('    fclose(out);', '    fclose(out); prof_encode_end=prof_now();'),
        ('    fprintf(stderr, "%d frames,', '    fprintf(stderr,"PROFILE %.9f %.9f %.9f %.9f\\n",prof_look_start-prof_start,prof_look_end-prof_look_start,prof_encode_end-prof_encode_start,prof_now()-prof_start);\n    fprintf(stderr, "%d frames,')]
    for old, new in replacements:
        if src.count(old) != 1:
            raise RuntimeError("profiling insertion no longer unique: " + old)
        src = src.replace(old, new)
    source = work / "profile_cpu.c"
    source.write_text(src)
    subprocess.run(["cc", "-O3", "-I", str(ROOT / "tools/tvc_ref"), "-o", str(work / "profile_cpu"), str(source), "-lm"], check=True)
    rows = []
    for case in ("action_400", "anime_dialog_400", "bw_room_400", "action_800"):
        clip = work / "ref" / (case + ".y4m")
        output = work / "profile-output.tvc"
        r = subprocess.run([str(work / "profile_cpu"), str(clip), str(output), "-q", "30", "-n", "96"], capture_output=True, text=True, check=True)
        match = re.search(r"PROFILE ([\d.]+) ([\d.]+) ([\d.]+) ([\d.]+)", r.stderr)
        if not match:
            raise RuntimeError(r.stderr)
        read, look, enc, total = map(float, match.groups())
        with clip.open("rb") as f:
            header = f.readline().decode()
        w, h = (int(re.search(pattern, header).group(1)) for pattern in (r" W(\d+)", r" H(\d+)"))
        mbs = ((w + 15) // 16) * ((h + 15) // 16) * 96
        row = dict(source_sha256=source_sha256, dependency_sha256=dependency_sha256, case=case, qp=30, frames=96, read_setup_s=read, lookahead_s=look, encode_s=enc, total_s=total,
                   lookahead_fraction=look/total, infinite_lookahead_speedup=total/(total-look), macroblocks=mbs,
                   encode_us_per_mb=enc*1e6/mbs, production_stream_checked=a.verify)
        if a.verify:
            production = work / "profile-production.tvc"
            subprocess.run([str(ROOT / "tools/tvc_ref/tvc_enc"), str(clip), str(production), "-q", "30", "-n", "96"], check=True, capture_output=True)
            row["stream_sha256"] = hashlib.sha256(output.read_bytes()).hexdigest()
            if row["stream_sha256"] != hashlib.sha256(production.read_bytes()).hexdigest():
                raise RuntimeError("instrumented output differs from production: " + case)
        rows.append(row)
        print(json.dumps(row), flush=True)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(rows, indent=2) + "\n")

if __name__ == "__main__":
    main()

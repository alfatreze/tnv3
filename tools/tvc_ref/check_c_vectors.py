"""Decode every vector with the C reference decoder and compare with vectors/expected.txt.
A line `name REJECT` is a negative vector: the decoder must refuse it with exit status 3 (level violation)."""
import hashlib, pathlib, subprocess, sys, tempfile
HERE = pathlib.Path(__file__).resolve().parent
bad = 0
with tempfile.TemporaryDirectory() as td:
    for line in (HERE / "vectors" / "expected.txt").read_text().split("\n"):
        if not line.strip(): continue
        name, md5 = line.split()
        out = pathlib.Path(td) / "o.y4m"
        r = subprocess.run([str(HERE / "tvc_dec"), str(HERE / "vectors" / name), str(out)], capture_output=True, text=True)
        if md5 == "REJECT":
            ok = r.returncode == 3; got = f"status {r.returncode}: {r.stderr.strip()}"
        else:
            ok = r.returncode == 0 and hashlib.md5(out.read_bytes()).hexdigest() == md5
            got = hashlib.md5(out.read_bytes()).hexdigest() if r.returncode == 0 else f"status {r.returncode}"
        bad += not ok
        print("PASS" if ok else "FAIL", name, got)
sys.exit(1 if bad else 0)

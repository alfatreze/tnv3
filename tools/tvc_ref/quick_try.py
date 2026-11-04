"""quick_try.py CLIP QP [encoder args...]: encode a reference clip (a study clip name, or a path to a .y4m), check enc
recon == C decoder output, print bytes/frame, VMAF, PSNR-Y and the encoder's summary line.  For quick development
checks; the full evaluation is eval_tvc.py."""
import hashlib, pathlib, re, struct, subprocess, sys, tempfile, time
HERE = pathlib.Path(__file__).resolve().parent
REF = pathlib.Path("<tau-v-checkout>/work/codec_phase0/ref")
clip, qp, extra = sys.argv[1], sys.argv[2], sys.argv[3:]
src = pathlib.Path(clip) if "/" in clip else REF / f"{clip}_800.y4m"
with tempfile.TemporaryDirectory() as td:
    td = pathlib.Path(td); tvc, rec, dec = td / "a.tvc", td / "r.y4m", td / "d.y4m"
    t0 = time.time()
    e = subprocess.run([str(HERE / "tvc_enc"), str(src), str(tvc), "-q", qp, "-r", str(rec), *extra], check=True, capture_output=True, text=True)
    t1 = time.time()
    subprocess.run([str(HERE / "tvc_dec"), str(tvc), str(dec)], check=True)
    same = hashlib.md5(rec.read_bytes()).hexdigest() == hashlib.md5(dec.read_bytes()).hexdigest()
    p = subprocess.run(["ffmpeg", "-hide_banner", "-i", str(dec), "-i", str(src), "-lavfi",
                        "[0:v]split[a][b];[1:v]split[c][d];[a][c]libvmaf=n_threads=4;[b][d]psnr", "-f", "null", "-"], capture_output=True, text=True)
    vm = re.search(r"VMAF score: ([0-9.]+)", p.stderr).group(1); ps = re.search(r"PSNR y:([0-9.]+)", p.stderr).group(1)
    n = struct.unpack("<I", tvc.read_bytes()[12:16])[0]
    nbytes = tvc.stat().st_size - 16 - 4 * n
    print(f"{src.stem} q{qp} {' '.join(extra)}: {nbytes / n:.0f} B/frame VMAF {vm} PSNR-Y {ps} enc {t1 - t0:.1f}s match={same} | {e.stderr.strip()}")

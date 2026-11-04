"""Build the TVC v0 conformance vectors: small encodes of the study clips plus an odd-size crop, decoded by the C
reference; writes vectors/*.tvc and vectors/expected.txt (md5 of the decoded y4m, and of the encoder's own recon,
which must be identical).  Usage: python3 make_vectors.py  (needs ./tvc_enc, ./tvc_dec, ffmpeg)"""
import hashlib, pathlib, subprocess, tempfile

HERE = pathlib.Path(__file__).resolve().parent
REF = pathlib.Path("<tau-v-checkout>/work/codec_phase0/ref")
OUT = HERE / "vectors"
CASES = [  # name, clip, frames, args, optional crop
    ("v01_dialog_ip", "dialog", 10, ["-q", "30", "-g", "4"], None),
    ("v02_anime_aq", "anime_motion", 8, ["-q", "26", "-g", "8", "-a", "1.0"], None),
    ("v03_bw_lowq", "bw_room", 6, ["-q", "44", "-g", "3"], None),
    ("v04_action_highq_aq", "action", 6, ["-q", "18", "-g", "6", "-a", "0.6"], None),
    ("v05_odd_crop", "dark", 7, ["-q", "28", "-g", "5", "-a", "1.0"], "crop=99:71:301:113"),
    # draft 2: B pictures (v01-v05 were made by encoder v1 and are kept unchanged: existing files are not re-encoded)
    ("v06_dialog_b2_tree", "dialog", 13, ["-q", "30", "-g", "12", "-b", "2", "-t", "1.0"], None),
    ("v07_anime_b3_gop7", "anime_motion", 16, ["-q", "28", "-g", "7", "-b", "3", "-t", "2.0"], None),
    ("v08_action_b1_aq", "action", 9, ["-q", "24", "-g", "96", "-b", "1", "-a", "0.8"], None),
    ("v09_odd_crop_b2", "bw_garden", 11, ["-q", "34", "-g", "6", "-b", "2", "-t", "1.0"], "crop=67:45:211:333"),
    # draft 3: quarter-sample motion
    ("v10_dialog_qpel", "dialog", 13, ["-q", "30", "-g", "12", "-b", "2", "-p", "1"], None),
    ("v11_action_qpel_hiq", "action", 10, ["-q", "20", "-g", "96", "-b", "3", "-p", "1"], None),
    ("v12_odd_crop_qpel", "anime_motion", 9, ["-q", "32", "-g", "5", "-b", "1", "-p", "1"], "crop=83:61:401:203"),
    # draft 4: 4x4 transform option (with scene-cut I pictures from the encoder's default analysis)
    ("v13_anime_t4", "anime_wide", 13, ["-q", "28", "-g", "12", "-x", "1"], None),
    ("v14_dialog_t4_hiq_iframes", "dialog", 7, ["-q", "16", "-g", "1", "-x", "1"], None),
    ("v15_odd_crop_t4", "bw_closeup", 10, ["-q", "30", "-g", "6", "-b", "2", "-x", "1", "-p", "0"], "crop=91:53:333:250"),
    # draft 5: slices and levels (TVC1 header)
    ("v16_dialog_l2_3slices", "dialog", 12, ["-q", "30", "-g", "12", "-l", "2", "-S", "7"], None),
    ("v17_bw_l3_2slices", "bw_room", 10, ["-q", "32", "-l", "3"], None),
    ("v18_anime_4row_slices_tvc0", "anime_motion", 10, ["-q", "28", "-S", "4"], None),
    ("v19_l1_crop_slices", "action", 9, ["-q", "26", "-l", "1", "-S", "3"], "crop=320:176:240:80"),
    ("v20_qp_max_aq", "action", 5, ["-q", "51", "-g", "5", "-a", "1.0"], "crop=160:96:200:80"),    # QP 51 reached (the upper clip of the QP delta)
    ("v21_qp_min_aq", "dialog", 5, ["-q", "0", "-g", "5", "-a", "1.0"], "crop=160:96:200:80"),     # QP 0 reached (the lower clip)
]
# negative vectors: must be REJECTED by a conforming decoder (C reference: exit status 3, "level violation")
NEGATIVE = [("n01_level1_too_wide", "v01_dialog_ip", 1),   # v01's pictures with a TVC1 header claiming level 1 (800 wide)
            ("n03_level3_one_slice", "v01_dialog_ip", 3)]  # same pictures claiming level 3: pictures have 1 slice, level 3 needs 2
# n02: a real level-1 encode whose pictures each fit (< 96,000 bytes) but whose one-second windows exceed 200,000 bytes
NEG_ENCODE = [("n02_level1_rate", "action", 30, ["-q", "8", "-g", "1", "-l", "1", "-R", "0"], "crop=320:176:240:80")]

def md5(p): return hashlib.md5(pathlib.Path(p).read_bytes()).hexdigest()

def main():
    OUT.mkdir(exist_ok=True); lines = []
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td)
        for name, clip, n, args, crop in CASES:
            src = REF / f"{clip}_800.y4m"
            if crop:
                c = td / f"{name}_src.y4m"
                subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(src), "-frames:v", str(n), "-vf", crop, "-pix_fmt", "yuv420p", str(c)], check=True)
                src = c
            tvc, rec, dec = OUT / f"{name}.tvc", td / f"{name}_rec.y4m", td / f"{name}_dec.y4m"
            fresh = not tvc.exists()
            if fresh:
                subprocess.run([str(HERE / "tvc_enc"), str(src), str(tvc), *args, "-n", str(n), "-r", str(rec)], check=True, capture_output=True)
            subprocess.run([str(HERE / "tvc_dec"), str(tvc), str(dec)], check=True)
            a, b = (md5(rec) if fresh else None), md5(dec)
            if fresh and a != b: raise SystemExit(f"{name}: encoder recon and decoder output differ")
            lines.append(f"{name}.tvc {b}")
            print(name, tvc.stat().st_size, "bytes", b)
        for name, clip, n, args, crop in NEG_ENCODE:
            if (OUT / f"{name}.tvc").exists(): continue
            c = td / f"{name}_src.y4m"
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(REF / f"{clip}_800.y4m"), "-frames:v", str(n), "-vf", crop, "-pix_fmt", "yuv420p", str(c)], check=True)
            subprocess.run([str(HERE / "tvc_enc"), str(c), str(OUT / f"{name}.tvc"), *args, "-n", str(n)], check=True, capture_output=True)
        for name, base, lvl in NEGATIVE + [(e[0], None, None) for e in NEG_ENCODE]:
            if base:
                data = (OUT / f"{base}.tvc").read_bytes()
                (OUT / f"{name}.tvc").write_bytes(b"TVC1" + data[4:16] + bytes([lvl, 0, 0, 0]) + data[16:])
            r = subprocess.run([str(HERE / "tvc_dec"), str(OUT / f"{name}.tvc"), str(td / "neg.y4m")], capture_output=True, text=True)
            if r.returncode != 3: raise SystemExit(f"{name}: expected rejection (status 3), got {r.returncode}")
            lines.append(f"{name}.tvc REJECT")
            print(name, "rejected:", r.stderr.strip())
    (OUT / "expected.txt").write_text("\n".join(lines) + "\n")

if __name__ == "__main__":
    main()

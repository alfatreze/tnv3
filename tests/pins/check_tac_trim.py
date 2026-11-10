#!/usr/bin/env python3
"""tac trim mode (-C checkpoints, -R/-F/-N) must equal encoding the trimmed WAV directly, byte for byte."""
import math, os, struct, subprocess, sys, tempfile, wave
root = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
tac = os.environ.get("TAU_TEST_TAC") or os.path.join(root, "tools", "tac_ref", "tac")
def wav(path, n, ch):
    w = wave.open(path, "wb"); w.setnchannels(ch); w.setsampwidth(2); w.setframerate(48000)
    seed = 12345; out = bytearray()
    for i in range(n):
        seed = (seed * 1103515245 + 12345) & 0x7fffffff
        burst = 9000 if (i // 24000) % 3 == 2 and i % 24000 < 600 else 0   # attacks, so short windows occur
        for c in range(ch):
            v = int(6000 * math.sin(i * (0.031 + 0.011 * c)) + 2500 * math.sin(i * 0.31) + burst * ((seed >> 8) % 3 - 1))
            out += struct.pack("<h", max(-32768, min(32767, v)))
    w.writeframes(bytes(out)); w.close()
def run(*a): subprocess.run([tac, *a], check=True, stderr=subprocess.DEVNULL)
fails = 0
with tempfile.TemporaryDirectory() as d:
    p = lambda n: os.path.join(d, n)
    for ch, kbps, total, cuts in ((2, 96, 150000, (149999, 120001, 70000, 33333, 4096, 1500)), (1, 64, 100000, (99000, 50001, 20000))):
        wav(p("src.wav"), total, ch)
        run("enc", p("src.wav"), p("full.tac"), "-b", str(kbps), "-C", p("full.ck"))
        for ns in cuts:
            src = wave.open(p("src.wav")); cut = wave.open(p("cut.wav"), "wb"); cut.setparams(src.getparams()); cut.writeframes(src.readframes(ns)); cut.close()
            run("enc", p("cut.wav"), p("direct.tac"), "-b", str(kbps))
            run("enc", p("src.wav"), p("trim.tac"), "-b", str(kbps), "-R", p("full.ck"), "-F", p("full.tac"), "-N", str(ns))
            ok = open(p("direct.tac"), "rb").read() == open(p("trim.tac"), "rb").read()
            print("ok  " if ok else "FAIL", f"{ch}ch {kbps} kbps cut at {ns} of {total}")
            fails += not ok
sys.exit(1 if fails else 0)

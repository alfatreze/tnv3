#!/usr/bin/env python3
"""TAC1 feature bit 1 (trained initial contexts) and the encoder options -D / -P: the decoders (tac and tac_fx) must give the same
samples for TAC0 and TAC1 when the quantised data are the same (a quality floor that never binds), and the default encode must still
write plain TAC0. Needs the tac and tac_fx tools."""
import math, os, struct, subprocess, sys, tempfile, wave
root = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
tac = os.environ.get("TAU_TEST_TAC") or os.path.join(root, "tools", "tac_ref", "tac")
tacfx = os.path.join(os.path.dirname(tac), "tac_fx")
def run(exe, *a): subprocess.run([exe, *a], check=True, stderr=subprocess.DEVNULL)
fails = 0
def check(ok, what):
    global fails
    print("ok  " if ok else "FAIL", what); fails += not ok
with tempfile.TemporaryDirectory() as d:
    p = lambda n: os.path.join(d, n)
    w = wave.open(p("s.wav"), "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
    seed = 7; out = bytearray()
    for i in range(96000):
        seed = (seed * 1103515245 + 12345) & 0x7fffffff
        for c in range(2):
            v = int(7000 * math.sin(i * (0.04 + 0.013 * c)) + 1800 * math.sin(i * 0.37) + ((seed >> 9) % 700 - 350))
            out += struct.pack("<h", v)
    w.writeframes(bytes(out)); w.close()
    run(tac, "enc", p("s.wav"), p("a.tac"), "-b", "96")
    check(open(p("a.tac"), "rb").read(4) == b"TAC0", "default encode writes TAC0")
    run(tac, "enc", p("s.wav"), p("b.tac"), "-b", "96", "-L", "6")
    run(tac, "enc", p("s.wav"), p("c.tac"), "-b", "96", "-L", "6", "-X", "1")
    check(open(p("c.tac"), "rb").read(4) == b"TAC1" and open(p("c.tac"), "rb").read(10)[9] == 1, "-X 1 writes TAC1 with feature bit 1")
    check(os.path.getsize(p("c.tac")) < os.path.getsize(p("b.tac")), "trained contexts make the file smaller")
    for exe in (tac, tacfx):
        run(exe, "dec", p("b.tac"), p("b.wav")); run(exe, "dec", p("c.tac"), p("c.wav"))
        check(open(p("b.wav"), "rb").read() == open(p("c.wav"), "rb").read(), f"{os.path.basename(exe)}: TAC0 and TAC1 decode to the same samples")
    bad = bytearray(open(p("c.tac"), "rb").read()); bad[9] = 2
    open(p("bad.tac"), "wb").write(bad)
    check(subprocess.run([tac, "dec", p("bad.tac"), p("x.wav")], stderr=subprocess.DEVNULL).returncode != 0, "unknown feature bits are refused")
    run(tac, "enc", p("s.wav"), p("e.tac"), "-b", "96", "-D", "0.28", "-P", "1")
    run(tac, "dec", p("e.tac"), p("e.wav"))
    check(open(p("e.tac"), "rb").read(4) == b"TAC0", "-D and -P are encoder-only: the stream stays plain TAC0")
    # -V 1 prints `P frame total` progress lines to stderr and changes nothing in the output
    r = subprocess.run([tac, "enc", p("s.wav"), p("v.tac"), "-b", "96", "-V", "1"], capture_output=True, text=True, check=True)
    lines = [l for l in r.stderr.splitlines() if l.startswith("P ")]
    check(len(lines) >= 2 and lines[0].split()[1] == "0" and all(len(l.split()) == 3 for l in lines), "-V 1 prints progress lines (P frame total)")
    check(open(p("v.tac"), "rb").read() == open(p("a.tac"), "rb").read(), "-V 1 does not change the encoded bytes")
sys.exit(1 if fails else 0)

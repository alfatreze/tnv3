"""Build the TAC v0 conformance vectors: short encodes chosen to exercise every syntax path, decoded by the C
reference.  Writes vectors/NAME.tac and vectors/NAME.ref.wav (the reference decoder's output; a conforming decoder must
match it within the tolerance of docs/TAC_BITSTREAM_V0.md section 7) and vectors/manifest.txt with what each covers.
Existing vectors are not re-encoded.  Usage: python3 make_vectors.py  (needs ./tac, ./tac_gen and ffmpeg)"""
import math, pathlib, struct, subprocess, tempfile, wave

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "vectors"
MEDIA = pathlib.Path("<tau-v-checkout>/media")

def synth(path, seconds=2.0, click_period=12000):
    """stereo test signal: a held chord (tonal, left/right correlated -> mid/side), clicks every 0.25 s (transients ->
    SHORT windows), and decaying noise bursts (noise filling)"""
    sr = 48000; n = int(seconds * sr); frames = bytearray()
    seed = 12345
    for i in range(n):
        t = i / sr
        tone = 0.25 * (math.sin(2 * math.pi * 440 * t) + 0.5 * math.sin(2 * math.pi * 660 * t) + 0.3 * math.sin(2 * math.pi * 5200 * t))
        ph = i % click_period
        click = (1.0 if ph < 40 else 0.0) * (1 if (ph // 4) % 2 else -1) * 0.8
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        noise = ((seed >> 8) / 2 ** 23 - 0.5) * 0.3 * math.exp(-((i % 24000) / 6000.0))
        l = tone + click + noise; r = tone * 0.9 + click - noise * 0.5
        for v in (l, r): frames += struct.pack("<h", max(-32768, min(32767, int(v * 20000))))
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(sr); w.writeframes(bytes(frames))

def stress(path):
    """3 s at full scale, for the integer model's limits: a 2-20 kHz sine sweep, a clipped 997 Hz square wave, white noise
    (channels inverted); encoded at a high rate it reaches magnitudes above 1024 and scale factor 255"""
    sr = 48000; frames = bytearray(); seed = 1
    for i in range(3 * sr):
        t = i / sr
        if t < 1: v = math.sin(2 * math.pi * (2000 * t + 9000 * t * t))
        elif t < 2: v = 1.0 if math.sin(2 * math.pi * 997 * t) >= 0 else -1.0
        else:
            seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF; v = seed / 2 ** 30 - 1
        x = max(-32768, min(32767, int(round(v * 32767))))
        frames += struct.pack("<hh", x, -x if t >= 2 else x)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(sr); w.writeframes(bytes(frames))

CASES = [  # name, source builder, encoder bitrate, what it is meant to cover
    ("t01_synth_stereo_96k", "synth", 96, "SHORT/START/STOP windows, per-band mid/side, noise filling, tonal bands"),
    ("t02_film_action_96k", ("tears", 436, 3, 2), 96, "real film audio, reset every 16 frames, bit reservoir"),
    ("t03_film_dialog_128k", ("tears", 355, 3, 2), 128, "128 kbit/s bandwidth (42 bands), dialog"),
    ("t04_tv_mono_64k", ("bw", 10, 3, 1), 64, "mono stream (no mid/side syntax), low rate: many all-zero bands"),
    ("t05_synth_lr_dense_clicks", ("synth", 1100), 96, "stereo left/right (msmode 0), clicks every 1100 samples: runs of SHORT frames", ["-M", "0"]),
    ("t06_synth_ms_all_320k", "synth", 320, "msmode 1 (mid/side in every band), high rate: large magnitudes / long escapes", ["-M", "1"]),
    ("t08_crafted_limits", "crafted", 0, "crafted by tac_gen (no encoder makes it): mid/side values beyond the +-2^26 limit, scale factor 255 with magnitude 8191, scale factor 0, loud noise filling"),
    ("t07_stress_fullscale_hi", "stress", 1200, "full-scale sweep / square / noise at a high rate: magnitudes above 1024, scale factor 255, coefficient clamp range"),
]

def main():
    OUT.mkdir(exist_ok=True); lines = []
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td)
        for case in CASES:
            name, src, kb, what = case[:4]; extra = case[4] if len(case) > 4 else []
            tac, ref = OUT / f"{name}.tac", OUT / f"{name}.ref.wav"
            if not tac.exists():
                wav = td / f"{name}.wav"
                if src == "crafted":
                    subprocess.run([str(HERE / "tac_gen"), str(tac)], check=True)
                    subprocess.run([str(HERE / "tac"), "dec", str(tac), str(ref)], check=True)
                    lines.append(f"{name}: {what}")
                    continue
                if src == "stress": stress(wav)
                elif src == "synth": synth(wav)
                elif src[0] == "synth": synth(wav, click_period=src[1])
                else:
                    which, ss, dur, ch = src
                    f = MEDIA / ("tears-of-steel.mp4" if which == "tears" else "MPEG-4 movie.mp4")
                    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(ss), "-t", str(dur), "-i", str(f), "-vn", "-ac", str(ch), "-ar", "48000", "-c:a", "pcm_s16le", str(wav)], check=True)
                e = subprocess.run([str(HERE / "tac"), "enc", str(wav), str(tac), "-b", str(kb), *extra], check=True, capture_output=True, text=True)
                print(name, e.stderr.strip())
            subprocess.run([str(HERE / "tac"), "dec", str(tac), str(ref)], check=True)
            lines.append(f"{name}: {what}")
    (OUT / "manifest.txt").write_text("\n".join(lines) + "\n")

if __name__ == "__main__":
    main()

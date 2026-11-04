#!/usr/bin/env python3
"""Blind listening pack for TAC v0 against MP3 (LAME) and Opus on three 30 s film/TV excerpts.

For each excerpt: ref.wav (the 48 kHz 16-bit original) and six coded versions, decoded back to WAV and named by a random
letter (A-F): TAC 96 kbit/s, TAC 128 kbit/s, TAC 128 kbit/s without the tonality model, MP3 128 kbit/s, Opus 64 kbit/s,
Opus 96 kbit/s.  The key is in key.json
(do not open it before ranking).  Also writes measured bit rates and the objective sanity numbers of audcmp.py to
notes.txt (not a perceptual measure).  Output: <worktree>/work/tac_listening/.
Usage: python3 listening_pack.py"""
import json, pathlib, random, subprocess, sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUT = ROOT / "work" / "tac_listening"
MEDIA = pathlib.Path("<tau-v-checkout>/media")
EXCERPTS = {   # name: (source, start s)
    "film_dialog": (MEDIA / "tears-of-steel.mp4", 355),
    "film_action": (MEDIA / "tears-of-steel.mp4", 436),
    # the anime episode's audio is damaged (30 s windows decode to 7-23 s), so the third excerpt is the B&W TV opening
    "tv_opening": (MEDIA / "MPEG-4 movie.mp4", 10),
}

def ff(*a): subprocess.run(["ffmpeg", "-v", "error", "-y", *a], check=True)

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    rng = random.Random(202610282)
    key, notes = {}, []
    for name, (src, ss) in EXCERPTS.items():
        d = OUT / name; d.mkdir(exist_ok=True)
        ref = d / "ref.wav"
        ff("-ss", str(ss), "-t", "30", "-i", str(src), "-vn", "-map", "0:a:0", "-ac", "2", "-ar", "48000", "-c:a", "pcm_s16le", str(ref))
        tmp = d / "_tmp"; tmp.mkdir(exist_ok=True)
        variants = {}
        for label, kb, extra in (("TAC 96k", 96, []), ("TAC 128k", 128, []), ("TAC 128k no-tonality", 128, ["-T", "0"])):
            tag = label.replace(" ", "_")
            t = tmp / f"{tag}.tac"
            subprocess.run([str(HERE / "tac"), "enc", str(ref), str(t), "-b", str(kb), *extra], check=True, capture_output=True)
            subprocess.run([str(HERE / "tac"), "dec", str(t), str(tmp / f"{tag}.wav")], check=True)
            variants[label] = (tmp / f"{tag}.wav", t.stat().st_size)
        for label, args, ext in (("MP3 128k", ["-c:a", "libmp3lame", "-b:a", "128k"], "mp3"),
                                 ("Opus 64k", ["-c:a", "libopus", "-b:a", "64k", "-vbr", "on"], "opus"),
                                 ("Opus 96k", ["-c:a", "libopus", "-b:a", "96k", "-vbr", "on"], "opus")):
            c = tmp / f"{label.replace(' ', '_')}.{ext}"
            ff("-i", str(ref), *args, str(c))
            w = tmp / f"{label.replace(' ', '_')}.wav"
            ff("-i", str(c), "-ar", "48000", "-ac", "2", "-c:a", "pcm_s16le", str(w))
            variants[label] = (w, c.stat().st_size)
        letters = list("ABCDEF"); rng.shuffle(letters)
        key[name] = {}
        for (label, (wav, size)), L in zip(variants.items(), letters):
            dst = d / f"{L}.wav"; wav.replace(dst)
            key[name][L] = label
            r = subprocess.run([sys.executable, str(HERE / "audcmp.py"), str(ref), str(dst)], capture_output=True, text=True)
            notes.append(f"{name} {L} ({label}): {size * 8 / 30 / 1000:.1f} kbit/s | {r.stdout.strip()}")
        for p in tmp.iterdir(): p.unlink()
        tmp.rmdir()
    (OUT / "key.json").write_text(json.dumps(key, indent=1))
    (OUT / "notes.txt").write_text("\n".join(notes) + "\n")
    (OUT / "README.txt").write_text(
        "Blind listening test, TAC v0 with the tonality model (2026-10-28, pack 2).\n"
        "Each folder: ref.wav = original; A..F = six coded versions in random order (TAC 96k, TAC 128k, TAC 128k without the\n"
        "tonality model, MP3 128k, Opus 64k, Opus 96k).\n"
        "Use headphones on the Pocket-like listening level you expect. For each folder rate A..E against ref (5 = same as ref,\n"
        "4 = audible but not annoying, 3 = slightly annoying, 2 = annoying, 1 = very annoying), and note what you heard\n"
        "(pre-echo on hits, swishy/watery highs, dull highs, stereo image, hiss). Open key.json only afterwards.\n"
        "notes.txt holds bit rates and objective numbers; reading it before listening would break the blind test.\n")
    print("pack written to", OUT)

if __name__ == "__main__":
    main()

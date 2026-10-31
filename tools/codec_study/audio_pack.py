#!/usr/bin/env python3
"""Blind listening pack: Opus vs MP3 (LAME) at several bitrates on two 60 s film excerpts.

No perceptual audio metric (PEAQ / ViSQOL) is installed, and plain SNR says nothing about
perceptual codecs, so this makes files for a listening test instead.  Every encode is decoded
back to 48 kHz stereo WAV and given a random letter; the key is written to key.json (do not
open it until you have ranked the files).  The reference is ref.wav.
"""
import json, pathlib, random, subprocess

SRC = pathlib.Path("<tau-v-worktree>/media/tears-of-steel.mp4")
OUT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0" / "audio"
EXCERPTS = {"dialog": 355, "action": 436}
ENCODERS = [   # label, codec args, extension
    ("opus48", ["-c:a", "libopus", "-b:a", "48k", "-vbr", "on", "-application", "audio"], "opus"),
    ("opus64", ["-c:a", "libopus", "-b:a", "64k", "-vbr", "on", "-application", "audio"], "opus"),
    ("opus96", ["-c:a", "libopus", "-b:a", "96k", "-vbr", "on", "-application", "audio"], "opus"),
    ("mp3_96", ["-c:a", "libmp3lame", "-b:a", "96k"], "mp3"),
    ("mp3_128", ["-c:a", "libmp3lame", "-b:a", "128k"], "mp3"),
    ("mp3_160", ["-c:a", "libmp3lame", "-b:a", "160k"], "mp3"),
]


def run(args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    rng = random.Random(20261020)
    key = {}
    for name, ss in EXCERPTS.items():
        d = OUT / name
        d.mkdir(exist_ok=True)
        ref = d / "ref.wav"
        run(["-ss", str(ss), "-t", "60", "-i", str(SRC), "-vn", "-ac", "2", "-ar", "48000", str(ref)])
        labels = list("ABCDEF")
        rng.shuffle(labels)
        key[name] = {}
        for (tag, args, ext), lab in zip(ENCODERS, labels):
            enc = d / f"enc_{tag}.{ext}"
            run(["-i", str(ref), *args, str(enc)])
            run(["-i", str(enc), "-ar", "48000", "-ac", "2", str(d / f"{lab}.wav")])
            key[name][lab] = {"codec": tag, "kbps": round(enc.stat().st_size * 8 / 60 / 1000, 1)}
            enc.unlink()
    (OUT / "key.json").write_text(json.dumps(key, indent=1, sort_keys=True))
    print("wrote", OUT)


if __name__ == "__main__":
    main()

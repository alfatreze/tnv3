#!/usr/bin/env python3
"""Smoke-test the C-reference helper pipeline on generated media.

This is a staging runner for CLI migration phase 1. It only reads the Tau
codecs checkout and puts generated media/artifacts in a temporary directory.
It does not invoke the Rust desktop application.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def resolve_executable(value: str, label: str) -> str:
    found = shutil.which(value)
    if not found:
        raise SystemExit(f"{label} executable not found: {value}")
    return str(Path(found).resolve())


def y4m_frame_count(path: Path) -> tuple[int, tuple[int, int, str]]:
    """Count framed Y4M payloads using stream dimensions/chroma, not byte scans."""
    with path.open("rb") as stream:
        header = stream.readline().rstrip(b"\n").decode("ascii")
        if not header.startswith("YUV4MPEG2 "):
            raise AssertionError(f"invalid Y4M header in {path}")
        fields = {field[0]: field[1:] for field in header.split()[1:] if len(field) > 1}
        try:
            width, height = int(fields["W"]), int(fields["H"])
            chroma = fields.get("C", "420jpeg")
        except (KeyError, ValueError) as exc:
            raise AssertionError(f"missing or invalid Y4M dimensions in {path}: {header}") from exc
        if chroma != "420jpeg":
            raise AssertionError(f"expected 8-bit 4:2:0 jpeg-range Y4M, got {chroma!r}")
        payload_bytes = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)
        count = 0
        while True:
            marker = stream.readline()
            if not marker:
                break
            if not marker.startswith(b"FRAME") or not marker.endswith(b"\n"):
                raise AssertionError(f"invalid Y4M frame marker at record {count}: {marker!r}")
            payload = stream.read(payload_bytes)
            if len(payload) != payload_bytes:
                raise AssertionError(f"short Y4M payload at frame {count}: {len(payload)} bytes")
            count += 1
        return count, (width, height, chroma)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--codec-root",
        type=Path,
        required=True,
    )
    ap.add_argument("--ffmpeg", default="ffmpeg")
    ap.add_argument("--ffprobe", default="ffprobe")
    ap.add_argument("--python", default="python3")
    args = ap.parse_args()
    ffmpeg = resolve_executable(args.ffmpeg, "ffmpeg")
    ffprobe = resolve_executable(args.ffprobe, "ffprobe")
    python = resolve_executable(args.python, "python")

    root = args.codec_root.resolve()
    suffix = ".exe" if os.name == "nt" else ""
    script = root / "tools/tnv3_ref/encode_film.py"
    binaries = {
        "TVC": root / "tools/tvc_ref" / f"tvc_enc{suffix}",
        "TNV3": root / "tools/tnv3_ref" / f"tnv3{suffix}",
        "C decoder": root / "tools/tvc_ref" / f"tvc_dec{suffix}",
    }
    for path in [script, *binaries.values()]:
        if not path.is_file():
            raise SystemExit(f"required read-only codec input is missing: {path}")
    with tempfile.TemporaryDirectory(prefix="tau-cli-phase1-") as temporary:
        temp = Path(temporary)
        source = temp / "generated-24-frame.mp4"
        subprocess.run(
            [
                ffmpeg,
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=1",
                "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=1",
                "-c:v", "mpeg4", "-q:v", "3", "-c:a", "aac", "-shortest",
                str(source),
            ],
            check=True,
        )

        video = temp / "desktop.tnv3"
        report = temp / "desktop.json"
        audio = temp / "desktop.mp3"
        work = temp / "segments"
        env = os.environ.copy()
        env.update(TAU_FFMPEG=ffmpeg,
                   TAU_FFPROBE=ffprobe,
                   TAU_TVC_ENC=str(binaries["TVC"]), TAU_TNV3=str(binaries["TNV3"]))
        print("Generated input: 64x48, 24 fps, 1 second, MPEG-4 video + AAC sine audio")
        print("Desktop video path: production C-reference encode_film.py --video-only --match-app")
        subprocess.run([python, str(script), str(source), str(video), "--video-only",
                        "--match-app", "--qp", "30", "--jobs", "1", "--cpu-budget", "1",
                        "--work", str(work)], cwd=root, env=env, check=True)
        data = json.loads(report.read_text()) if report.exists() else None
        # encode_film writes its report beside the requested output as .json.
        if data is None:
            raise AssertionError("desktop helper did not produce its JSON report")
        if data.get("frames") != 24:
            raise AssertionError(f"expected 24 output frames, got {data.get('frames')}")
        subprocess.run([str(binaries["TNV3"]), "check", str(video)], check=True)
        video_tvc = temp / "desktop.tvc"
        subprocess.run([str(binaries["TNV3"]), "demux", str(video), str(video_tvc)],
                       check=True, capture_output=True)
        decoded = temp / "desktop-decoded.y4m"
        subprocess.run([str(binaries["C decoder"]), str(video_tvc), str(decoded)],
                       check=True, capture_output=True)
        actual_frames, dimensions = y4m_frame_count(decoded)
        if actual_frames != 24:
            raise AssertionError(f"C decoder produced {actual_frames} frames, expected 24")
        if dimensions != (400, 300, "420jpeg"):
            raise AssertionError(f"unexpected decoded Pocket dimensions/chroma: {dimensions}")
        expected_duration = f"{24 / 24:.9f}"
        subprocess.run([ffmpeg, "-y", "-nostdin", "-hide_banner", "-loglevel", "error",
                        "-i", str(source), "-map", "0:a:0", "-vn", "-map_metadata", "-1",
                        "-af", "apad", "-t", expected_duration, "-ar", "48000", "-ac", "2", "-c:a",
                        "libmp3lame", "-b:a", "128k", "-f", "mp3", str(audio)],
                       check=True, capture_output=True)
        probe = subprocess.run([ffprobe, "-v", "error", "-select_streams", "a:0",
                                "-show_entries", "stream=codec_name,sample_rate,channels",
                                "-of", "default=nw=1", str(audio)], check=True,
                               capture_output=True, text=True).stdout
        if not all(item in probe for item in ("codec_name=mp3", "sample_rate=48000", "channels=2")):
            raise AssertionError(f"unexpected desktop MP3 stream metadata: {probe}")
        decoded_audio = subprocess.run([ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                                       "-i", str(audio), "-f", "s16le", "-ac", "2", "-ar",
                                       "48000", "pipe:1"], check=True, capture_output=True).stdout
        audio_sample_frames = len(decoded_audio) // 4
        if len(decoded_audio) % 4 or abs(audio_sample_frames - 48000) > 1152:
            raise AssertionError(
                f"decoded MP3 duration is {audio_sample_frames} stereo sample frames; expected ~48000"
            )
        diagnostics = subprocess.run([ffmpeg, "-nostdin", "-hide_banner", "-loglevel",
                                      "warning", "-i", str(source), "-map", "0:v:0", "-map",
                                      "0:a:0", "-f",
                                      "null", "-"], capture_output=True, text=True, check=True).stderr
        if diagnostics.strip():
            raise AssertionError(f"clean generated video/audio produced decoder warnings: {diagnostics}")
        print("PASS: 24-frame desktop-rate video, C TNV3 check and decode, MP3 48 kHz stereo")
        print("PASS: strict warning-free FFmpeg decode of generated clean source")
        print("LIMIT: Tauri UI/Rust preview parity and warning collection are not exercised")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())

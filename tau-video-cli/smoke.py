#!/usr/bin/env python3
"""Generated-input host smoke test for the staged video-only CLI."""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent


def executable(value: str) -> str:
    found = shutil.which(value)
    if not found:
        raise SystemExit(f"executable not found: {value}")
    # Keep the command's symlink name (cargo commonly points through rustup).
    return str(Path(os.path.abspath(found)))


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codec-root", type=Path, required=True)
    parser.add_argument("--codec-tools", type=Path,
                        help="directory containing tools/tvc_ref and tools/tnv3_ref executables")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    root = args.codec_root.resolve()
    tools = args.codec_tools.resolve() if args.codec_tools else root / "tools"
    cargo = executable(args.cargo)
    ffmpeg = executable(args.ffmpeg)
    ffprobe = executable(args.ffprobe)
    py = Path(os.sys.executable).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    script = root / "tools/tnv3_ref/encode_film.py"
    tvc = tools / "tvc_ref" / f"tvc_enc{suffix}"
    tnv3 = tools / "tnv3_ref" / f"tnv3{suffix}"
    for path in (script, tvc, tnv3):
        if not path.is_file():
            raise SystemExit(f"required codec dependency missing: {path}")

    manifest = HERE / "Cargo.toml"
    with tempfile.TemporaryDirectory(prefix="tau-video-cli-smoke-") as td:
        work = Path(td)
        source, output = work / "input.mp4", work / "video.tnv3"
        subprocess.run([ffmpeg, "-y", "-hide_banner", "-loglevel", "error",
                        "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24:duration=1",
                        "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=1",
                        "-c:v", "mpeg4", "-q:v", "3", "-c:a", "aac", "-shortest",
                        str(source)], check=True)
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(work / "target")
        subprocess.run([cargo, "test", "--offline", "--manifest-path", str(manifest)], env=env, check=True)
        command = [cargo, "run", "--offline", "--quiet", "--manifest-path", str(manifest), "--",
                   "encode", str(source), str(output), "--python", str(py), "--script", str(script),
                   "--ffmpeg", ffmpeg, "--ffprobe", ffprobe, "--tvc-encoder", str(tvc),
                   "--tnv3-tool", str(tnv3), "--qp", "30", "--jobs", "1", "--video-only"]
        subprocess.run(command, env=env, check=True)
        if output.with_suffix(".mp3").exists():
            raise AssertionError("--video-only unexpectedly created an MP3 sidecar")
        subprocess.run([str(tnv3), "check", str(output)], check=True)
        info = subprocess.run([str(tnv3), "info", str(output)], check=True, capture_output=True, text=True).stdout
        if "pictures 24" not in info:
            raise AssertionError(f"TNV3 checker info did not report 24 pictures: {info}")
        before = output.read_bytes()
        overwrite = subprocess.run(command, env=env, capture_output=True, text=True)
        if overwrite.returncode == 0 or "Output already exists" not in overwrite.stderr:
            raise AssertionError("CLI must reject an existing output path")
        if output.read_bytes() != before:
            raise AssertionError("existing output changed after overwrite was rejected")
        audio = subprocess.run([ffprobe, "-v", "error", "-select_streams", "a:0", "-show_entries",
                                "stream=codec_name", "-of", "default=nw=1", str(output)],
                               capture_output=True, text=True)
        if audio.returncode == 0 and "codec_name=" in audio.stdout:
            raise AssertionError("video-only TNV3 unexpectedly contains an audio stream")
        print("PASS: CLI encoded 24-frame video-only TNV3; checker accepted it; overwrite rejected")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

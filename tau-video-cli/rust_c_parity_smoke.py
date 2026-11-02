#!/usr/bin/env python3
"""Opt-in 481-frame parity check for the experimental streamed Rust C backend."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


HERE = Path(__file__).resolve().parent


def executable(value: str) -> str:
    found = shutil.which(value)
    if not found:
        raise SystemExit(f"executable not found: {value}")
    return str(Path(os.path.abspath(found)))


def run(command: list[str], env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(command, env=env, capture_output=True, text=True)
    for output in (result.stdout, result.stderr):
        for line in output.splitlines():
            if "warning" in line.lower() or "fallback" in line.lower():
                print(f"TOOL_WARNING: {line}")
    if result.returncode:
        raise RuntimeError(
            f"command failed ({result.returncode}): {command!r}\n{result.stdout}\n{result.stderr}"
        )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codec-root", type=Path, required=True,
                        help="Tau codecs checkout with encode_film.py and C tools")
    parser.add_argument("--codec-tools", type=Path,
                        help="directory containing tools/tvc_ref and tools/tnv3_ref")
    parser.add_argument("--ffmpeg", required=True)
    parser.add_argument("--ffprobe", required=True)
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--python", default=sys.executable)
    args = parser.parse_args()

    root = args.codec_root.resolve()
    codec_tools = args.codec_tools.resolve() if args.codec_tools else root / "tools"
    suffix = ".exe" if os.name == "nt" else ""
    python = executable(args.python)
    ffmpeg = executable(args.ffmpeg)
    ffprobe = executable(args.ffprobe)
    cargo = executable(args.cargo)
    script = root / "tools/tnv3_ref/encode_film.py"
    tvc = codec_tools / "tvc_ref" / f"tvc_enc{suffix}"
    tnv3 = codec_tools / "tnv3_ref" / f"tnv3{suffix}"
    for path in (script, tvc, tnv3):
        if not path.is_file():
            raise SystemExit(f"required codec dependency missing: {path}")

    with tempfile.TemporaryDirectory(prefix="tau-rust-c-parity-481-") as temp:
        work = Path(temp)
        source = work / "fixture-481.mp4"
        python_output = work / "python.tnv3"
        rust_output = work / "rust.tnv3"
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(work / "target")
        run([ffmpeg, "-y", "-hide_banner", "-loglevel", "error",
             "-f", "lavfi", "-i", "testsrc2=size=64x48:rate=24",
             "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000",
             "-frames:v", "481", "-c:v", "mpeg4", "-q:v", "3",
             "-c:a", "aac", "-shortest", str(source)], env)
        probe = run([ffprobe, "-v", "error", "-count_frames", "-select_streams", "v:0",
                     "-show_entries", "stream=nb_read_frames,r_frame_rate", "-of", "json",
                     str(source)], env)
        video = json.loads(probe.stdout)["streams"][0]
        if int(video["nb_read_frames"]) != 481 or video["r_frame_rate"] != "24/1":
            raise AssertionError(f"generated fixture is not 481 frames at 24 fps: {video}")

        run([cargo, "build", "--locked", "--offline", "--manifest-path",
             str(HERE / "Cargo.toml")], env)
        cli = work / "target" / "debug" / f"tau-codecs-cli{suffix}"
        common = ["--ffmpeg", ffmpeg, "--ffprobe", ffprobe, "--tvc-encoder", str(tvc),
                  "--tnv3-tool", str(tnv3), "--qp", "30", "--jobs", "2"]
        py = run([str(cli), "encode", str(source), str(python_output), "--python", python,
                  "--script", str(script), *common], env)
        rust = run([str(cli), "encode", str(source), str(rust_output),
                    "--rust-c-orchestrator", *common], env)
        if not re.search(r"segments: 2/2", py.stdout + py.stderr):
            raise AssertionError(
                f"Python path did not report two segments: {py.stdout}\n{py.stderr}"
            )
        if not re.search(r"2 segment\(s\)", rust.stdout):
            raise AssertionError(f"Rust path did not report two segments: {rust.stdout}")
        for output in (python_output, rust_output):
            run([str(tnv3), "check", str(output)], env)
            info = run([str(tnv3), "info", str(output)], env).stdout
            if "pictures 481" not in info:
                raise AssertionError(f"TNV3 checker did not report 481 pictures: {info}")
        python_mp3 = python_output.with_suffix(".mp3")
        rust_mp3 = rust_output.with_suffix(".mp3")
        if python_output.read_bytes() != rust_output.read_bytes():
            raise AssertionError("Python and Rust TNV3 outputs differ")
        if python_mp3.read_bytes() != rust_mp3.read_bytes():
            raise AssertionError("Python and Rust MP3 sidecars differ")
        print("PASS: 481-frame 24 fps fixture; both paths used two segments; TNV3 and MP3 bytes match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

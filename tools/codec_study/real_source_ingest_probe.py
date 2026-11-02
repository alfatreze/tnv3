#!/usr/bin/env python3
"""Aggregate first-minute FFmpeg parity check; never saves source/frame hashes."""
import argparse
import json
import pathlib
import subprocess
import threading
from fractions import Fraction
from hashlib import md5

FILTERS = {
    "source_rate": "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,format=yuv420p",
    "default": "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24,format=yuv420p",
    "round_down": "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24:round=down,format=yuv420p",
    "round_down_eof_pass": "scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24:round=down:eof_action=pass,format=yuv420p",
}


def invoke(argv):
    return subprocess.run(argv, capture_output=True, text=True)


def check_probe(ffprobe, source):
    result = invoke([ffprobe, "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", str(source)])
    streams = json.loads(result.stdout).get("streams", []) if result.returncode == 0 else []
    kinds = {stream.get("codec_type") for stream in streams}
    if not {"video", "audio"}.issubset(kinds):
        raise RuntimeError("ffprobe could not confirm media streams")


def read_y4m(stdout):
    header = stdout.readline().decode("ascii", errors="strict").strip().split()
    if not header or header[0] != "YUV4MPEG2":
        raise RuntimeError("FFmpeg did not emit a YUV4MPEG2 header")
    fields = {field[0]: field[1:] for field in header[1:] if len(field) > 1}
    width, height = int(fields["W"]), int(fields["H"])
    rate = Fraction(fields["F"].replace(":", "/"))
    if width % 2 or height % 2:
        raise RuntimeError("Expected even-sized 4:2:0 Y4M frames")
    frame_size = width * height * 3 // 2
    rows = []
    while True:
        marker = stdout.readline()
        if not marker:
            break
        if not marker.startswith(b"FRAME"):
            raise RuntimeError("Invalid Y4M frame header")
        payload = stdout.read(frame_size)
        if len(payload) != frame_size:
            raise RuntimeError("Truncated Y4M frame")
        rows.append((len(rows), md5(payload).hexdigest()))
    return rows, rate


def describe(rows, rate, stderr):
    repeated = [index for index in range(1, len(rows)) if rows[index][1] == rows[index - 1][1]]
    frame_duration = float(1 / rate)
    return {"frame_count": len(rows), "frame_rate": f"{rate.numerator}/{rate.denominator}",
            "last_cfr_frame_index": rows[-1][0] if rows else None,
            "last_cfr_position_seconds": rows[-1][0] * frame_duration if rows else None,
            "adjacent_identical_pixel_pairs": len(repeated),
            "stderr_error_lines": sum(bool(line.strip()) for line in stderr.splitlines())}


def capture(ffmpeg, source, vf, seconds, input_cutoff):
    command = [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error"]
    command += (["-t", str(seconds), "-i", str(source)] if input_cutoff
                else ["-i", str(source), "-t", str(seconds)])
    command += ["-map", "0:v:0", "-an", "-sn", "-vf", vf, "-pix_fmt", "yuv420p", "-f", "yuv4mpegpipe", "pipe:1"]
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stderr_buffer = bytearray()
    def drain_stderr():
        assert child.stderr is not None
        while True:
            chunk = child.stderr.read(8192)
            if not chunk:
                break
            stderr_buffer.extend(chunk)
    thread = threading.Thread(target=drain_stderr, daemon=True)
    thread.start()
    assert child.stdout is not None
    rows, rate = read_y4m(child.stdout)
    child.stdout.close()
    status = child.wait()
    thread.join()
    if status:
        raise RuntimeError(f"FFmpeg failed with exit {status}")
    return (rows, rate), bytes(stderr_buffer).decode("utf-8", errors="replace")


def compare(left, right, left_rate, right_rate):
    shared = min(len(left), len(right))
    differing = [i for i in range(shared) if left[i] != right[i]]
    first = differing[0] if differing else None
    return {"same_frame_count": len(left) == len(right), "same_frame_rate": left_rate == right_rate,
            "same_pixel_sequence_at_frame_indices": [row[1] for row in left] == [row[1] for row in right],
            "shared_positions_with_pixel_difference": len(differing),
            "first_differing_cfr_frame_index": first,
            "first_differing_left_cfr_position_seconds": first * float(1 / left_rate) if first is not None else None,
            "frame_count_delta_left_minus_right": len(left) - len(right)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--ffmpeg-old", required=True)
    parser.add_argument("--ffmpeg-new", required=True)
    parser.add_argument("--ffprobe", required=True)
    parser.add_argument("--seconds", type=int, default=60)
    parser.add_argument("--output", default="comparison-aggregate.json")
    args = parser.parse_args()
    source = pathlib.Path(args.source)
    if not source.is_file() or args.seconds < 1:
        parser.error("source must be readable and seconds must be positive")

    check_probe(args.ffprobe, source)
    results = {"scope_seconds": args.seconds,
               "method": {"filter_stages": "app scale -> fps candidate -> yuv420p",
                          "timestamp_evidence": "Y4M has no per-frame PTS; positions are inferred from frame index and CFR header rate",
                          "timestamp_filters_or_manual_reset": "none; no setpts, copyts, or explicit start-time reset",
                          "input_cutoff_note": "input_cutoff is an artificial bounded test window, not the source file EOF",
                          "log_level": "error (production-equivalent)"}, "runs": {}, "comparisons": {}}
    frames = {}
    for engine, binary in (("old", args.ffmpeg_old), ("new", args.ffmpeg_new)):
        version = invoke([binary, "-version"])
        version_line = version.stdout.splitlines()[0] if version.returncode == 0 else "version unavailable"
        for window in ("output_window", "input_cutoff"):
            for mode, vf in FILTERS.items():
                (rows, rate), stderr = capture(binary, source, vf, args.seconds, window == "input_cutoff")
                key = f"{engine}/{window}/{mode}"
                frames[key] = rows
                results["runs"][key] = {"ffmpeg_version": version_line, "mode": mode, "window": window,
                                        **describe(rows, rate, stderr)}

    keys = list(frames)
    for index, left_key in enumerate(keys):
        for right_key in keys[index + 1:]:
            left = frames[left_key]
            right = frames[right_key]
            left_rate = Fraction(results["runs"][left_key]["frame_rate"])
            right_rate = Fraction(results["runs"][right_key]["frame_rate"])
            result = compare(left, right, left_rate, right_rate)
            results["comparisons"][f"{left_key} vs {right_key}"] = result

    output = pathlib.Path(args.output)
    with output.open("x", encoding="utf-8") as report:
        json.dump(results, report, indent=2)
        report.write("\n")
    for key, run in results["runs"].items():
        print(key, json.dumps({name: run[name] for name in ("frame_count", "frame_rate", "last_cfr_frame_index",
              "last_cfr_position_seconds", "adjacent_identical_pixel_pairs", "stderr_error_lines")}))
    print(f"Aggregate report saved to {output}")


if __name__ == "__main__":
    main()

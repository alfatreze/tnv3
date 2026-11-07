"""Host-only generated-media smoke for schema-1 native CLI events."""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parent
binary = Path(os.environ.get('TAU_TEST_CLI', root / 'target/debug/tau-codecs-cli'))
ffmpeg = os.environ['TAU_TEST_FFMPEG']
tvc = os.environ['TAU_TEST_TVC']
tnv3 = os.environ['TAU_TEST_TNV3']
ffprobe = os.environ['TAU_TEST_FFPROBE']
script = os.environ['TAU_TEST_C_SCRIPT']
with tempfile.TemporaryDirectory(prefix='tau-json-smoke-') as directory:
    base = Path(directory)
    source = base / 'input video.mkv'
    subprocess.run([ffmpeg, '-nostdin', '-v', 'error', '-f', 'lavfi', '-i',
        'testsrc2=size=64x48:rate=24:duration=1', '-f', 'lavfi', '-i',
        'sine=frequency=440:sample_rate=48000:duration=1', '-c:v', 'ffv1',
        '-c:a', 'pcm_s16le', '-shortest', str(source)], check=True)
    for backend, extra, expected_kind in [
        ('native', ['--rust-c-orchestrator'], 'joint_segments'),
        ('python', ['--python', os.environ.get('TAU_TEST_PYTHON', sys.executable), '--script', script, '--ffprobe', ffprobe], 'pictures_raised_for_level'),
    ]:
        output = base / f'{backend}.tnv3'
        result = subprocess.run([str(binary), 'encode', str(source), str(output), *extra,
            '--ffmpeg', ffmpeg, '--tvc-encoder', tvc, '--tnv3-tool', tnv3, '--json-events'],
            text=True, capture_output=True, timeout=60)
        if result.returncode:
            raise RuntimeError(f'{backend} CLI failed ({result.returncode}): {result.stderr}\n{result.stdout}')
        events = [json.loads(line) for line in result.stdout.splitlines()]
        assert events and all(event['schema'] == 1 for event in events)
        assert events[0]['event'] == 'plan' and events[-1]['event'] == 'done'
        assert any(event['event'] == 'progress' for event in events)
        done = events[-1]
        files = [Path(name) for name in done['files']]
        assert files == [output, output.with_suffix('.mp3')]
        assert done['sizes'] == [path.stat().st_size for path in files]
        assert done['sha256'] == [hashlib.sha256(path.read_bytes()).hexdigest() for path in files]
        assert done['reencoded'] == 0 and done['reencoded_kind'] == expected_kind
        subprocess.run([tnv3, 'check', str(output)], check=True, capture_output=True)
        subprocess.run([ffmpeg, '-nostdin', '-v', 'error', '-i', str(files[1]), '-f', 'null', '-'],
                       check=True, capture_output=True)
        print(f'{backend} JSON smoke passed: {len(events)} events, TNV3 and MP3 checked, hashes match.')

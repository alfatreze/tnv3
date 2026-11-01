#!/usr/bin/env python3
"""Compare three encoders on generated 400x226 media; no user media required."""
import argparse, json, pathlib, re, statistics, struct, subprocess, tempfile, time
ap = argparse.ArgumentParser()
for name in ('ffmpeg', 'c-encoder', 'c-decoder', 'rust-before', 'rust-after'):
    ap.add_argument('--' + name, required=True)
a = ap.parse_args()
def run(args):
    return subprocess.run(args, check=True, capture_output=True)
results = {}
with tempfile.TemporaryDirectory(prefix='tau-parity-') as folder:
    root = pathlib.Path(folder); src = root / 'input.y4m'
    run([a.ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=400x226:rate=24:duration=1', '-pix_fmt', 'yuv420p', '-f', 'yuv4mpegpipe', '-y', str(src)])
    configs = {'rust_before': [a.rust_before, 'encode-tvc'], 'rust_after': [a.rust_after, 'encode-tvc'], 'c_reference': [a.c_encoder]}
    for name, prefix in configs.items():
        durations = []; samples = []
        for i in range(3):
            out = root / f'{name}-{i}.tvc'
            cmd = prefix + [str(src), str(out)] + (['-q', '30', '-l', '1'] if name == 'c_reference' else [])
            start = time.monotonic(); run(cmd); durations.append(time.monotonic() - start); samples.append(out.read_bytes())
        if not all(x == samples[0] for x in samples): raise RuntimeError(f'{name}: nondeterministic output')
        data = samples[0]; offset = 20 if data[:4] == b'TVC1' else 16; counts = {'I': 0, 'P': 0, 'B': 0}
        for _ in range(struct.unpack_from('<I', data, 12)[0]):
            length = struct.unpack_from('<I', data, offset)[0]; counts[{0:'I', 1:'P', 2:'B'}[data[offset+4]]] += 1; offset += 4 + length
        decoded = root / f'{name}.y4m'; run([a.c_decoder, str(root / f'{name}-0.tvc'), str(decoded)])
        psnr = run([a.ffmpeg, '-i', str(decoded), '-i', str(src), '-lavfi', 'psnr', '-f', 'null', '-']).stderr.decode(errors='replace')
        metric = re.search(r'PSNR y:([\d.]+).*average:([\d.]+)', psnr)
        if not metric: raise RuntimeError('PSNR unavailable')
        results[name] = {'bytes': len(data), 'picture_types': counts, 'psnr_y_db': float(metric.group(1)), 'psnr_yuv_db': float(metric.group(2)), 'median_seconds': round(statistics.median(durations), 4), 'runs_seconds': [round(x, 4) for x in durations]}
print(json.dumps({'source': 'generated testsrc2 400x226 24fps 1s, QP30', 'note': 'same nominal QP, not equal quality; standalone TVC encoders, short host sample', 'encoders': results}, indent=2))

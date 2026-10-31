#!/usr/bin/env python3
"""Single-run longer host measurements; macOS wait4 reports each process peak RSS."""
import hashlib, json, os, pathlib, platform, re, subprocess, tempfile, time
root = pathlib.Path(__file__).resolve().parents[3]
work = root / 'work/encoder_lab'
binaries = [('before', pathlib.Path(os.environ.get('TVC_LONG_BEFORE', str(work / 'encoder-original'))).resolve()),
            ('after', pathlib.Path(os.environ.get('TVC_LONG_AFTER', str(root / 'tools/tvc_ref/tvc_enc'))).resolve())]
if any(not binary.is_relative_to(root) for _, binary in binaries):
 raise ValueError('encoders must be inside the worktree')
cases = [('action_400', 30), ('anime_dialog_400', 24), ('bw_room_400', 36), ('action_800', 30)]
rows = []
with tempfile.TemporaryDirectory(dir=work) as td:
 for case, qp in cases:
  for label, binary in binaries:
   out = pathlib.Path(td) / 'stream.tvc'
   cmd = [str(binary), str(work / 'ref' / (case + '.y4m')), str(out), '-q', str(qp), '-n', '240']
   recon = pathlib.Path(td) / 'recon.y4m'
   if os.environ.get('TVC_LONG_RECON') == '1': cmd += ['-r', str(recon)]
   start = time.perf_counter()
   with tempfile.TemporaryFile(mode='w+') as err:
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=err)
    _, status, usage = os.wait4(p.pid, 0); p.returncode = os.waitstatus_to_exitcode(status)
    seconds = time.perf_counter() - start; err.seek(0); diagnostic = err.read()
   if p.returncode: raise RuntimeError(diagnostic)
   frames = int(re.search(r'(\d+) frames,', diagnostic).group(1)); assert frames == 240
   rss = usage.ru_maxrss
   row = dict(case=case, qp=qp, encoder=label, frames=frames, seconds=seconds, fps=frames/seconds, peak_rss_bytes=rss, stream_sha256=hashlib.sha256(out.read_bytes()).hexdigest(), recon_sha256=hashlib.sha256(recon.read_bytes()).hexdigest() if os.environ.get('TVC_LONG_RECON') == '1' else None, bytes=out.stat().st_size, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
   rows.append(row); print(json.dumps(row), flush=True)
pathlib.Path(os.environ.get('TVC_LONG_OUT', str(root / 'docs/encoder_long_baseline.json'))).write_text(json.dumps(dict(platform=platform.platform(), measurement='one run each; macOS wait4 peak RSS; full encoder including input and output', samples=rows), indent=2)+'\n')

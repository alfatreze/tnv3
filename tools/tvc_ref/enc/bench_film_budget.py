#!/usr/bin/env python3
"""Compare encoder-slot budgets on a 240-frame study film using the experimental pool."""
import contextlib, hashlib, importlib.util, io, json, pathlib, statistics, sys, tempfile, time
root = pathlib.Path(__file__).resolve().parents[3]
work = root / 'work/encoder_lab'
configs = [(2,1), (2,2), (4,1), (4,2)]
rows = []
with tempfile.TemporaryDirectory(prefix='film-budget-', dir=work) as td:
 for rep in range(3):
  for budget, threads in (configs if rep % 2 == 0 else list(reversed(configs))):
   spec = importlib.util.spec_from_file_location('budget_film', root / 'tools/tnv3_ref/encode_film.py')
   film = importlib.util.module_from_spec(spec); spec.loader.exec_module(film)
   film.TAC = str(work/'tac'); film.TNV3 = str(work/'tnv3'); film.TVC_ENC = str(work/'encoder-bpool')
   out = pathlib.Path(td)/'out.tnv3'; wd = pathlib.Path(td)/'segments'
   sys.argv = ['encode_film.py', str(work/'film-800-input.mkv'), str(out), '--width', '800', '--seg', '96', '--jobs', '4', '--cpu-budget', str(budget), '--encoder-threads', str(threads), '--work', str(wd)]
   start=time.perf_counter()
   with contextlib.redirect_stdout(io.StringIO()):
    try: film.main()
    except SystemExit as e:
     if e.code: raise
   seconds=time.perf_counter()-start
   report=json.loads(out.with_suffix('.json').read_text())
   assert report['valid'] and report['frames']==240 and report['encoder_jobs']==budget//threads and report['encoder_threads_requested']==threads
   row=dict(repetition=rep, budget=budget, threads=threads, jobs=report['encoder_jobs'], seconds=seconds, sha256=hashlib.sha256(out.read_bytes()).hexdigest(), report=report)
   rows.append(row); print(json.dumps({k:v for k,v in row.items() if k!='report'}),flush=True)
assert len({row['sha256'] for row in rows}) == 1
summary=[dict(budget=b, threads=t, jobs=b//t, median_seconds=statistics.median(row['seconds'] for row in rows if row['budget']==b and row['threads']==t)) for b,t in configs]
(root/'docs/encoder_film_budget.json').write_text(json.dumps(dict(samples=rows,summary=summary),indent=2)+'\n')

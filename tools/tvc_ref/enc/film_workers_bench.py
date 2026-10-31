import importlib.util,pathlib,sys,time,json,statistics,hashlib,contextlib,io
p=pathlib.Path.cwd(); samples=[]
for rep in range(3):
 for jobs in ([1,2,4] if rep%2==0 else [4,2,1]):
  for label,binary in ([('serial','tools/tvc_ref/tvc_enc'),('workers','work/encoder_lab/encoder-bthreads')] if rep%2==0 else [('workers','work/encoder_lab/encoder-bthreads'),('serial','tools/tvc_ref/tvc_enc')]):
   spec=importlib.util.spec_from_file_location('film','tools/tnv3_ref/encode_film.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
   m.TAC=str(p/'work/encoder_lab/tac');m.TNV3=str(p/'work/encoder_lab/tnv3');m.TVC_ENC=str((p/binary).resolve())
   out=p/'work/encoder_lab/film-workers-bench.tnv3';w=p/'work/encoder_lab/film-workers-bench-work'
   sys.argv=['encode_film.py',str(p/'work/encoder_lab/film-800-input.mkv'),str(out),'--width','800','--seg','96','--jobs',str(jobs),'--work',str(w)]
   start=time.perf_counter()
   with contextlib.redirect_stdout(io.StringIO()):
    try:m.main()
    except SystemExit as e:
     if e.code:raise
   elapsed=time.perf_counter()-start; report=json.loads(out.with_suffix('.json').read_text());assert report['valid'] and report['frames']==240
   samples.append(dict(rep=rep,jobs=jobs,encoder=label,seconds=elapsed,sha256=hashlib.sha256(out.read_bytes()).hexdigest()))
   print(samples[-1],flush=True)
assert len({s['sha256'] for s in samples})==1
summary=[dict(jobs=j,serial=statistics.median(s['seconds'] for s in samples if s['jobs']==j and s['encoder']=='serial'),workers=statistics.median(s['seconds'] for s in samples if s['jobs']==j and s['encoder']=='workers')) for j in [1,2,4]]
(p/'docs/encoder_film_workers_bench.json').write_text(json.dumps(dict(samples=samples,summary=summary),indent=2)+'\n')

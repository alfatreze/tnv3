#!/usr/bin/env python3
"""Repeated study input: compare segment/picture scheduling and whole-pipeline time."""
import contextlib, hashlib, importlib.util, io, json, pathlib, statistics, sys, tempfile, time
root=pathlib.Path(__file__).resolve().parents[3];work=root/'work/encoder_lab'
src=work/'film-400-repeated40.mkv';binary=work/'encoder-bsafe'
configs=[(96,2,1),(96,4,1),(96,4,2),(480,2,1),(480,4,1),(480,4,2)]
rows=[]
with tempfile.TemporaryDirectory(prefix='pipeline-study-',dir=work) as td:
 for rep in range(2):
  for seg,budget,threads in (configs if rep==0 else list(reversed(configs))):
   spec=importlib.util.spec_from_file_location('film_pipeline',root/'tools/tnv3_ref/encode_film.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
   m.TVC_ENC=str(binary);m.TAC=str(work/'tac');m.TNV3=str(work/'tnv3')
   out=pathlib.Path(td)/'film.tnv3'; directory=pathlib.Path(td)/'work'
   sys.argv=['encode_film.py',str(src),str(out),'--width','400','--seg',str(seg),'--jobs','4','--cpu-budget',str(budget),'--encoder-threads',str(threads),'--work',str(directory)]
   t=time.perf_counter()
   with contextlib.redirect_stdout(io.StringIO()):
    try:m.main()
    except SystemExit as e:
     if e.code:raise
   elapsed=time.perf_counter()-t;report=json.loads(out.with_suffix('.json').read_text())
   assert report['valid'] and report['frames']==960 and report['encoder_jobs']==budget//threads
   row=dict(rep=rep,seg=seg,budget=budget,threads=threads,jobs=report['encoder_jobs'],seconds=elapsed,sha256=hashlib.sha256(out.read_bytes()).hexdigest(),report=report)
   rows.append(row);print(json.dumps({k:v for k,v in row.items() if k!='report'}),flush=True)
for seg in [96,480]:
 assert len({r['sha256'] for r in rows if r['seg']==seg})==1
summary=[dict(seg=s,budget=b,threads=t,jobs=b//t,median_seconds=statistics.median(r['seconds'] for r in rows if (r['seg'],r['budget'],r['threads'])==(s,b,t))) for s,b,t in configs]
result=dict(scope='960-frame synthetic repetition of the same 10-second study clip; not a full-length film',source_sha256=hashlib.sha256(src.read_bytes()).hexdigest(),encoder_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),samples=rows,summary=summary)
(root/'docs/encoder_pipeline_repeated.json').write_text(json.dumps(result,indent=2)+'\n')

#!/usr/bin/env python3
"""Compare actual segment pipeline with synchronous scheduling on repeated study input."""
import concurrent.futures as cf, contextlib, hashlib, importlib.util, io, json, pathlib, statistics, sys, tempfile, time
root=pathlib.Path(__file__).resolve().parents[3];work=root/'work/encoder_lab';src=work/'film-400-repeated40.mkv';rows=[]
class ImmediatePool:
 def __init__(self, max_workers):self.max_workers=max_workers
 def submit(self,fn,*args,**kwargs):
  f=cf.Future()
  try:f.set_result(fn(*args,**kwargs))
  except BaseException as e:f.set_exception(e)
  return f
with tempfile.TemporaryDirectory(prefix='overlap-study-',dir=work) as td:
 for rep in range(2):
  for mode in (['pipeline','synchronous'] if rep==0 else ['synchronous','pipeline']):
   spec=importlib.util.spec_from_file_location('film_overlap',root/'tools/tnv3_ref/encode_film.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
   m.TVC_ENC=str(root/'tools/tvc_ref/tvc_enc');m.TAC=str(work/'tac');m.TNV3=str(work/'tnv3')
   out=pathlib.Path(td)/'film.tnv3';wd=pathlib.Path(td)/'work';original=cf.ThreadPoolExecutor
   sys.argv=['encode_film.py',str(src),str(out),'--width','400','--seg','96','--jobs','1','--work',str(wd)]
   start=time.perf_counter()
   try:
    if mode=='synchronous':cf.ThreadPoolExecutor=ImmediatePool
    with contextlib.redirect_stdout(io.StringIO()):
     try:m.main()
     except SystemExit as e:
      if e.code:raise
   finally:cf.ThreadPoolExecutor=original
   elapsed=time.perf_counter()-start;report=json.loads(out.with_suffix('.json').read_text());assert report['valid'] and report['frames']==960
   row=dict(rep=rep,mode=mode,seconds=elapsed,sha256=hashlib.sha256(out.read_bytes()).hexdigest(),report=report);rows.append(row);print(mode,elapsed,row['sha256'],flush=True)
assert len({r['sha256'] for r in rows})==1
summary={mode:statistics.median(r['seconds'] for r in rows if r['mode']==mode) for mode in ['pipeline','synchronous']}
(root/'docs/encoder_pipeline_overlap.json').write_text(json.dumps(dict(scope='960 frames by repeating one 10-second study clip; one TVC job; synchronous mode disables overlap of segment encode with frame ingestion and audio',source_sha256=hashlib.sha256(src.read_bytes()).hexdigest(),samples=rows,summary=summary),indent=2)+'\n')

#!/usr/bin/env python3
"""Validate two study-film paths against prior accepted complete TNV3 files."""
import hashlib, importlib.util, json, pathlib, sys
root=pathlib.Path(__file__).resolve().parents[3];work=root/'work/encoder_lab';rows=[]
for width,source,previous in [(400,'film-input.mkv','film-rdoq-prefix.tnv3'),(800,'film-800-input.mkv','film-800-rdoq-prefix.tnv3')]:
 spec=importlib.util.spec_from_file_location('film',root/'tools/tnv3_ref/encode_film.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 m.TAC=str(work/'tac');m.TNV3=str(work/'tnv3');m.TVC_ENC=str(root/'tools/tvc_ref/tvc_enc')
 out=work/f'film-final-{width}.tnv3'
 sys.argv=['encode_film.py',str(work/source),str(out),'--width',str(width),'--seg','96','--jobs','2','--work',str(work/f'film-final-{width}-work')]
 try:m.main()
 except SystemExit as e:
  if e.code:raise
 report=json.loads(out.with_suffix('.json').read_text());assert report['valid'] and report['frames']==240
 before=hashlib.sha256((work/previous).read_bytes()).hexdigest();after=hashlib.sha256(out.read_bytes()).hexdigest();assert before==after
 rows.append(dict(width=width,source=source,previous=previous,sha256=after,report=report))
(root/'docs/encoder_final_films.json').write_text(json.dumps(rows,indent=2)+'\n')

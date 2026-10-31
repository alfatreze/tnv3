#!/usr/bin/env python3
"""Compare B-QP offsets on held-out read-only study clips, without changing defaults."""
import hashlib, importlib.util, json, os, pathlib
root=pathlib.Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('encoder_lab',root/'tools/encoder_lab.py')
lab=importlib.util.module_from_spec(spec); spec.loader.exec_module(lab)
clips=['anime_motion','anime_wide','bw_garden','dark'];qps=[24,30,36];rows=[];summary=[]
for clip in clips:
 for offset in [2,3]:
  os.environ['TVC_ENCODER_ARGS']=f'-o {offset}'
  for qp in qps:
   row=lab.one(clip,qp,False);row['b_offset']=offset
   if not row['ok']:raise RuntimeError(row)
   rows.append(row);print(clip,offset,qp,row['bytes'],row['psnr_y'],flush=True)
 a=[r for r in rows if r['clip']==clip and r['b_offset']==2];b=[r for r in rows if r['clip']==clip and r['b_offset']==3]
 summary.append(dict(clip=clip,bd_rate=lab.bd_rate([(r['bytes'],r['psnr_y']) for r in a],[(r['bytes'],r['psnr_y']) for r in b]),equal_quality_bin_change=lab.bd_rate([(r['bins'],r['psnr_y']) for r in a],[(r['bins'],r['psnr_y']) for r in b])))
info=dict(scope='held-out study clips; 96 frames; offsets 2 versus 3; fixed QPs 24,30,36',encoder_sha256=hashlib.sha256(lab.ENCODER.read_bytes()).hexdigest(),source_sha256={c:hashlib.sha256((lab.REF/f'{c}_400.y4m').read_bytes()).hexdigest() for c in clips},results=rows,summary=summary)
(root/'docs/encoder_offset_holdout.json').write_text(json.dumps(info,indent=2)+'\n')

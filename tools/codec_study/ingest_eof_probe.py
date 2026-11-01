import argparse,importlib.util,json,tempfile
from pathlib import Path

def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod
ap=argparse.ArgumentParser();ap.add_argument('--bundled',required=True);ap.add_argument('--generator',required=True);args=ap.parse_args();root=Path(__file__).resolve().parent;v=load('video',root/'ingest_probe.py');s=load('sync',root/'ingest_sync_probe.py');b=args.bundled;g=args.generator;result={'fps':[],'sync':[]};policies={'current':'fps=24','down':'fps=24:round=down','down_pass':'fps=24:round=down:eof_action=pass','reset':'setpts=PTS-STARTPTS,fps=24'}
cases=[('hevc8','libx265','yuv420p','24',False),('hevc10','libx265','yuv420p10le','24',False),('avc8','libx264','yuv420p','24',False)]+[('hevc_'+r.replace('/','_'),'libx265','yuv420p10le',r,False) for r in ('24000/1001','25','30','12','15','60','30000/1001')]+[('offset','libx265','yuv420p','24',True)]
with tempfile.TemporaryDirectory(prefix='tau-eof-study-') as d:
 d=Path(d)
 for name,enc,pix,rate,off in cases:
  p=d/(name+'.mkv');v.make_source(g,p,enc,pix,rate,off);ref=v.y4m_frames(g,p); row={'name':name,'reference_count':len(ref),'policies':{}}
  for label,filt in policies.items():
   a=v.y4m_frames(b,p,filt);row['policies'][label]={'frames':len(a),'repeats':sum(x==y for x,y in zip(a,a[1:])),'equals_reference':a==ref}
  result['fps'].append(row)
 for name,offset,vfr in [('zero',0,False),('video_late',0.25,False),('video_early',-0.25,False),('variable_rate',0,True)]:
  p=d/(name+'-sync.mkv');s.source(g,p,offset,vfr);a=s.audio_marker(b,p);row={'name':name,'audio_marker':a,'policies':{}}
  for label,filt in policies.items():
   n,t=s.video_marker(b,p,filt);row['policies'][label]={'frames':n,'video_marker':t,'difference':round(t-a,6) if t is not None and a is not None else None}
  result['sync'].append(row)
print(json.dumps(result,indent=2))

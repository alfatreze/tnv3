#!/usr/bin/env python3
"""Research-only SW/VideoToolbox ingestion comparison. Uses prepared compressed study clips.
Run outside the sandbox for actual hardware access. Production film module is unchanged.
"""
import argparse,hashlib,importlib.util,json,pathlib,platform,statistics,subprocess,sys,tempfile,time
ROOT=pathlib.Path(__file__).resolve().parents[3];WORK=ROOT/'work/encoder_lab'
ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--phase',choices=['decode','film'],required=True);ap.add_argument('--out',type=pathlib.Path,required=True);ap.add_argument('--repeats',type=int,default=3);ap.add_argument('--prepare',action='store_true');o=ap.parse_args()
if o.repeats<1:ap.error('repeats must be positive')
if o.prepare:
 lossless=WORK/'film-800-input.mkv'
 subprocess.run(['ffmpeg','-v','error','-y','-i',str(WORK/'ref/action_800.y4m'),'-f','lavfi','-i','anullsrc=r=48000:cl=stereo','-map','0:v','-map','1:a','-c:v','ffv1','-c:a','pcm_s16le','-shortest',str(lossless)],check=True)
 for codec in ['h264','hevc']:
  for wide in [False,True]:
   cmd=['ffmpeg','-v','error','-y','-i',str(lossless),'-map','0:v','-map','0:a']
   if wide:cmd+=['-vf','scale=1920:802:flags=lanczos,setsar=1']
   cmd+=['-c:v','libx264' if codec=='h264' else 'libx265','-threads','2','-preset','fast','-crf','18' if codec=='h264' else '20','-pix_fmt','yuv420p']
   if codec=='hevc':cmd+=['-x265-params','pools=2:frame-threads=2']
   cmd+=['-c:a','copy',str(WORK/('ingest-'+codec+('-1920' if wide else '')+'.mkv'))];subprocess.run(cmd,check=True)
samples=[];summaries=[]
names=['ingest-h264','ingest-hevc','ingest-h264-1920','ingest-hevc-1920'] if o.phase=='decode' else ['ingest-h264-1920','ingest-hevc-1920']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def film(src,mode,directory):
 spec=importlib.util.spec_from_file_location('ingest_film',ROOT/'tools/tnv3_ref/encode_film.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 m.TAC=str(WORK/'tac');m.TNV3=str(WORK/'tnv3');m.TVC_ENC=str(ROOT/'tools/tvc_ref/tvc_enc')
 output=directory/'result.tnv3';original=subprocess.Popen;actual=[]
 def popen(args,*aa,**kk):
  if isinstance(args,list) and args[0]=='ffmpeg' and 'yuv4mpegpipe' in args:
   args=list(args)
   if mode=='vt':
    at=args.index('-i');args[at:at]=['-hwaccel','videotoolbox','-hwaccel_output_format','videotoolbox_vld'];at=args.index('-vf')+1;args[at]='hwdownload,format=nv12,'+args[at]
   actual.append(args)
  return original(args,*aa,**kk)
 subprocess.Popen=popen
 sys.argv=['encode_film.py',str(src),str(output),'--width','800','--seg','96','--jobs','2','--work',str(directory/'segments'),'--keep']
 start=time.perf_counter()
 try:
  try:m.main()
  except SystemExit as e:
   if e.code:raise
 finally:subprocess.Popen=original
 seconds=time.perf_counter()-start
 rep=json.loads(output.with_suffix('.json').read_text())
 assert rep['valid'] and rep['frames']==240 and rep['level']==2
 subprocess.run([m.TNV3,'check',str(output)],check=True,capture_output=True)
 return dict(seconds=seconds,sha256=sha(output),bytes=output.stat().st_size,frames=rep['frames'],level=rep['level'],video_command=actual,report=rep)
for name in names:
 src=WORK/(name+'.mkv');source_sha=sha(src)
 for repetition in range(o.repeats):
  for mode in (['sw','vt'] if repetition%2==0 else ['vt','sw']):
   with tempfile.TemporaryDirectory(prefix='ingest-trial-',dir=WORK) as td:
    directory=pathlib.Path(td)
    if o.phase=='decode':
     output=directory/'frames.yuv';vf='fps=24,scale=800:334:flags=lanczos,format=yuv420p'
     accel=['-hwaccel','none'] if mode=='sw' else ['-hwaccel','videotoolbox','-hwaccel_output_format','videotoolbox_vld']
     if mode=='vt':vf='hwdownload,format=nv12,'+vf
     cmd=['ffmpeg','-v','error','-y',*accel,'-i',str(src),'-map','0:v:0','-an','-vf',vf,'-fps_mode','passthrough','-f','rawvideo',str(output)]
     start=time.perf_counter();r=subprocess.run(cmd,capture_output=True,check=True);elapsed=time.perf_counter()-start
     assert output.stat().st_size==800*334*3//2*240
     row=dict(seconds=elapsed,sha256=sha(output),bytes=output.stat().st_size,frames=240,command=cmd)
    else:row=film(src,mode,directory)
    row.update(source=name,source_sha256=source_sha,mode=mode,repetition=repetition);samples.append(row);print(name,mode,row['seconds'],row['sha256'],flush=True)
 rows=[r for r in samples if r['source']==name];same=len({r['sha256'] for r in rows})==1
 med={mode:statistics.median(r['seconds'] for r in rows if r['mode']==mode) for mode in ['sw','vt']}
 summaries.append(dict(source=name,median_seconds=med,speedup=med['sw']/med['vt'],identical_output=same))
 # Save evidence before failing a pixel/range or frame-order equivalence check.
 o.out.parent.mkdir(parents=True,exist_ok=True);o.out.write_text(json.dumps(dict(phase=o.phase,platform=platform.platform(),scope='240-frame compressed study derivatives, not original feature-film files; 1920 input is upscaled from 800-pixel source',samples=samples,summary=summaries),indent=2)+'\n')
 if not same:raise RuntimeError('SW/VT output mismatch: '+name)
print(json.dumps(summaries,indent=2),flush=True)

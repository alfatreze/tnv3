#!/usr/bin/env python3
"""Measure FFmpeg MP3 gapless trimming with generated audio/video markers.

This compares FFmpeg's normal decoder with its skip_manual mode. It is a host
model, not a measurement of Helix or Pocket hardware.
"""
import argparse, array, importlib.util, json, subprocess, sys, tempfile
from pathlib import Path
ap=argparse.ArgumentParser();ap.add_argument('--bundled',required=True);ap.add_argument('--generator',required=True);a=ap.parse_args()
spec=importlib.util.spec_from_file_location('sync',Path(__file__).resolve().parent/'ingest_sync_probe.py');sync=importlib.util.module_from_spec(spec);spec.loader.exec_module(sync)
def marker(ffmpeg,path,manual):
    opts=['-flags2','+skip_manual'] if manual else []
    raw=subprocess.run([ffmpeg,'-v','error',*opts,'-i',str(path),'-ac','2','-ar','48000','-f','s16le','-'],capture_output=True,check=True).stdout
    pcm=array.array('h');pcm.frombytes(raw)
    if sys.byteorder!='little':pcm.byteswap()
    first=next((i//2 for i in range(0,len(pcm),2) if max(abs(pcm[i]),abs(pcm[i+1]))>1000),None)
    return {'first_pulse_sample':first,'decoded_sample_pairs':len(pcm)//2}
result={'cases':[]}
with tempfile.TemporaryDirectory(prefix='tau-mp3-marker-') as tmp:
 for name,offset in [('zero',0),('video_late',0.25),('video_early',-0.25)]:
    src=Path(tmp)/(name+'.mkv');mp3=Path(tmp)/(name+'.mp3');sync.source(a.generator,src,offset,False);frames,video=sync.video_marker(a.bundled,src,'fps=24')
    subprocess.run([a.bundled,'-v','error','-y','-i',str(src),'-map','0:a:0','-vn','-map_metadata','-1','-af','apad','-t',str(frames/24),'-ar','48000','-ac','2','-c:a','libmp3lame','-b:a','128k','-f','mp3',str(mp3)],capture_output=True,check=True)
    normal=marker(a.bundled,mp3,False);manual=marker(a.bundled,mp3,True)
    result['cases'].append({'name':name,'video_marker_seconds':video,'normal':normal,'skip_manual':manual,'pulse_difference_samples':manual['first_pulse_sample']-normal['first_pulse_sample']})
print(json.dumps(result,indent=2))

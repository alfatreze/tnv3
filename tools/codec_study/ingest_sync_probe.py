#!/usr/bin/env python3
"""Generate timestamp/audio markers and measure desktop ingest policies.

Requires an FFmpeg with libx265, plus the application's bundled FFmpeg:
python3 ingest_sync_probe.py --bundled /path/to/ffmpeg --generator /path/to/ffmpeg
All inputs are generated in a temporary folder and never published.
"""
import argparse,json,subprocess,tempfile
from pathlib import Path

def checked(args): return subprocess.run(args,check=True,capture_output=True)

def source(ffmpeg,path,offset,vfr):
    video="color=c=black:s=160x90:r=24:d=3,drawbox=x=0:y=0:w=160:h=90:color=white:t=fill:enable='between(t,1,1.12)'"
    if vfr: video+=",select='not(eq(mod(n,5),0))'"
    audio="aevalsrc=if(between(t\\,1\\,1.12)\\,0.8*sin(2*PI*1000*t)\\,0):s=48000:d=3"
    cmd=[ffmpeg,'-v','error','-y','-itsoffset',str(offset),'-f','lavfi','-i',video,'-f','lavfi','-i',audio,'-map','0:v','-map','1:a','-c:v','libx265','-preset','fast','-x265-params','pools=1:frame-threads=1:log-level=error','-pix_fmt','yuv420p10le']
    if vfr:cmd+=['-fps_mode','vfr']
    checked(cmd+['-c:a','pcm_s16le','-avoid_negative_ts','disabled',str(path)])

def audio_marker(ffmpeg,path):
    import array,sys
    b=checked([ffmpeg,'-v','error','-i',str(path),'-map','0:a:0','-ac','2','-ar','48000','-f','s16le','-']).stdout
    pcm=array.array('h');pcm.frombytes(b)
    if sys.byteorder!='little':pcm.byteswap()
    return next((i/96000 for i in range(0,len(pcm),960) if any(abs(x)>1000 for x in pcm[i:i+960])),None)

def video_marker(ffmpeg,path,fps):
    b=checked([ffmpeg,'-v','error','-i',str(path),'-map','0:v:0','-an','-vf','scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,'+fps,'-pix_fmt','yuv420p','-f','yuv4mpegpipe','-']).stdout
    i=b.index(b'\n');parts=b[:i].split();w=int(next(x[1:] for x in parts if x.startswith(b'W')));h=int(next(x[1:] for x in parts if x.startswith(b'H')));pixels=w*h;size=pixels+2*((w+1)//2)*((h+1)//2);i+=1;frames=0;flash=None
    while i<len(b):
        i=b.index(b'\n',i)+1; plane=b[i:i+pixels]
        if len(plane)!=pixels:raise ValueError('truncated frame')
        if flash is None and sum(plane)/pixels>150:flash=frames/24
        frames+=1;i+=size
    return frames,flash

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--bundled',required=True);ap.add_argument('--generator',required=True);a=ap.parse_args();out={'cases':[]}
    with tempfile.TemporaryDirectory(prefix='tau-sync-probe-') as tmp:
        for name,offset,vfr in [('zero',0,False),('video_late',0.25,False),('video_early',-0.25,False),('variable_rate',0,True)]:
            p=Path(tmp)/(name+'.mkv');source(a.generator,p,offset,vfr);audio=audio_marker(a.bundled,p)
            for label,fps in [('current','fps=24'),('round_down','fps=24:round=down'),('reset_pts','setpts=PTS-STARTPTS,fps=24')]:
                count,video=video_marker(a.bundled,p,fps);out['cases'].append({'name':name,'input_video_offset_seconds':offset,'variable_rate':vfr,'policy':label,'output_frames':count,'video_marker_seconds':video,'audio_marker_seconds':audio,'video_minus_audio_seconds':round(video-audio,6) if video is not None and audio is not None else None})
    print(json.dumps(out,indent=2))
if __name__=='__main__':main()

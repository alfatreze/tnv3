#!/usr/bin/env python3
"""Reproduce HEVC/H.264 ingest tests with generated material; no private media.

Requires a generator FFmpeg with libx264/libx265 and ffprobe. Example:
  python3 ingest_probe.py --bundled /path/to/app/ffmpeg --generator /opt/homebrew/bin/ffmpeg --ffprobe /opt/homebrew/bin/ffprobe
Output is aggregate JSON; temporary media is removed on exit.
"""
import argparse, hashlib, json, subprocess, tempfile
from pathlib import Path

FILTER = 'scale=400:360:force_original_aspect_ratio=decrease:force_divisible_by=2,'

def run(args):
    return subprocess.run(args, capture_output=True, check=True)

def y4m_frames(ffmpeg, source, fps='fps=24'):
    p = run([ffmpeg, '-v', 'error', '-i', str(source), '-map', '0:v:0', '-an', '-sn', '-vf', FILTER + fps, '-pix_fmt', 'yuv420p', '-f', 'yuv4mpegpipe', '-'])
    data = p.stdout; offset = data.index(b'\n'); fields = data[:offset].split()
    w = int(next(x[1:] for x in fields if x.startswith(b'W'))); h = int(next(x[1:] for x in fields if x.startswith(b'H')))
    size = w*h + 2*((w+1)//2)*((h+1)//2); offset += 1; hashes=[]
    while offset < len(data):
        end = data.index(b'\n', offset)
        if not data[offset:end].startswith(b'FRAME'): raise ValueError('Invalid frame marker')
        offset=end+1; frame=data[offset:offset+size]
        if len(frame)!=size: raise ValueError('Truncated frame')
        hashes.append(hashlib.sha256(frame).digest()); offset += size
    return hashes

def raw_hashes(ffmpeg, source):
    p=run([ffmpeg,'-v','error','-i',str(source),'-map','0:v:0','-an','-fps_mode','passthrough','-f','framemd5','-'])
    return [x.split(b',')[-1].strip().decode() for x in p.stdout.splitlines() if not x.startswith(b'#')]

def make_source(ffmpeg,path,encoder,pixfmt,rate,offset=False):
    video=(['-itsoffset','0.25'] if offset else [])+['-f','lavfi','-i',f'testsrc2=size=160x90:rate={rate}:duration=3']
    cmd=[ffmpeg,'-v','error','-y',*video,'-f','lavfi','-i',f'sine=frequency=440:sample_rate=48000:duration={3.5 if offset else 3}','-c:v',encoder,'-preset','fast']
    if encoder=='libx265': cmd+=['-x265-params','pools=1:frame-threads=1:log-level=error']
    cmd+=['-pix_fmt',pixfmt,'-c:a','aac']
    if not offset:cmd+=['-shortest']
    run(cmd+[str(path)])

def describe(seq,ref):
    return {'frames':len(seq),'adjacent_repeats':sum(a==b for a,b in zip(seq,seq[1:])), 'same_as_generator_default':seq==ref}

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--bundled',required=True);ap.add_argument('--generator',required=True);ap.add_argument('--ffprobe',required=True);a=ap.parse_args()
    results={'generator_version':run([a.generator,'-version']).stdout.decode().splitlines()[0], 'bundled_version':run([a.bundled,'-version']).stdout.decode().splitlines()[0], 'cases':[], 'mutation':{}}
    cases=[('hevc8','libx265','yuv420p','24',False),('hevc10','libx265','yuv420p10le','24',False),('avc8','libx264','yuv420p','24',False)]
    cases += [('hevc10_'+r.replace('/','_'),'libx265','yuv420p10le',r,False) for r in ('24000/1001','25','30','12','15','60','30000/1001')]
    cases += [('hevc_offset','libx265','yuv420p','24',True)]
    with tempfile.TemporaryDirectory(prefix='tau-hevc-audit-') as tmp:
        root=Path(tmp)
        for name,enc,pix,rate,offset in cases:
            p=root/(name+'.mkv');make_source(a.generator,p,enc,pix,rate,offset)
            ref=y4m_frames(a.generator,p)
            baseline=y4m_frames(a.bundled,p);down=y4m_frames(a.bundled,p,'fps=24:round=down');reset=y4m_frames(a.bundled,p,'setpts=PTS-STARTPTS,fps=24')
            results['cases'].append({'name':name,'source_rate':rate,'video_offset_seconds':0.25 if offset else 0,'native_decode_equal':raw_hashes(a.bundled,p)==raw_hashes(a.generator,p),'bundled_default':describe(baseline,ref),'bundled_round_down':describe(down,ref),'bundled_reset_pts':describe(reset,ref),'generator_frames':len(ref)})
            if name != 'hevc8':continue
            packets=json.loads(run([a.ffprobe,'-v','error','-select_streams','v:0','-show_packets','-show_entries','packet=pos,size,flags','-of','json',str(p)]).stdout)['packets']
            chosen=[x for x in packets if 'K' not in x['flags'] and int(x['size'])>200];pkt=chosen[len(chosen)//2];pos=int(pkt['pos']);size=int(pkt['size']);data=bytearray(p.read_bytes());start=pos+size//3;length=min(64,size//4);data[start:start+length]=b'\0'*length;q=root/'silent.mkv';q.write_bytes(data)
            x=raw_hashes(a.bundled,p);y=raw_hashes(a.bundled,q)
            results['mutation']['silent']={'bytes_changed':length,'clean_frames':len(x),'damaged_frames':len(y),'different_frames':sum(u!=v for u,v in zip(x,y)),'first_different_frame':next((i for i,(u,v) in enumerate(zip(x,y)) if u!=v),None)}
            pkt=next(x for x in packets[15:] if 'K' not in x['flags'] and int(x['size'])>500);pos=int(pkt['pos']);data=bytearray(p.read_bytes());data[pos+4:pos+12]=b'\xff'*8;q=root/'reported.mkv';q.write_bytes(data)
            for strict in (False,True):
                command=[a.bundled,'-v','error']+(['-xerror'] if strict else [])+['-i',str(q),'-map','0:v:0','-f','null','-'];proc=subprocess.run(command,capture_output=True)
                results['mutation']['reported_strict' if strict else 'reported_default']={'exit_code':proc.returncode,'stderr_nonempty':bool(proc.stderr),'stderr_lines':len(proc.stderr.splitlines())}
    print(json.dumps(results,indent=2))
if __name__=='__main__':main()

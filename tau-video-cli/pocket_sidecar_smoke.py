from pathlib import Path
import json, os, subprocess, tempfile
import argparse

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--codec-root',type=Path,required=True)
parser.add_argument('--ffmpeg',required=True)
parser.add_argument('--ffprobe',required=True)
parser.add_argument('--codec-tools',type=Path)
parser.add_argument('--tvc-decoder',type=Path)
parser.add_argument('--cargo',default='cargo')
args=parser.parse_args()
root=args.codec_root.resolve()
ffmpeg=str(Path(args.ffmpeg).resolve()); ffprobe=str(Path(args.ffprobe).resolve())
tools=args.codec_tools.resolve() if args.codec_tools else root/'tools'
script=str(tools/'tnv3_ref/encode_film.py')
tvc=str(tools/'tvc_ref/tvc_enc'); tvc_dec=str(args.tvc_decoder.resolve() if args.tvc_decoder else root/'tools/tvc_ref/tvc_dec'); tnv3=str(tools/'tnv3_ref/tnv3')
py=os.sys.executable
cli_manifest=Path(__file__).resolve().parent/'Cargo.toml'
def run(command, **kw):
    return subprocess.run(command, check=True, text=True, capture_output=True, **kw)
with tempfile.TemporaryDirectory(prefix='tau-pocket-pair-e2e-') as td:
    d=Path(td); src=d/'av.mp4'; silent=d/'silent.mp4'; out=d/'film.tnv3'
    env=os.environ.copy(); env['CARGO_TARGET_DIR']=str(d/'target')
    subprocess.run([args.cargo,'build','--offline','--manifest-path',str(cli_manifest)],check=True,env=env,capture_output=True,text=True)
    cli=str(d/'target/debug/tau-codecs-cli')
    run([ffmpeg,'-y','-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=64x48:rate=24:duration=1','-f','lavfi','-i','sine=frequency=440:sample_rate=48000:duration=1','-c:v','mpeg4','-q:v','3','-c:a','aac','-shortest',str(src)])
    run([ffmpeg,'-y','-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=64x48:rate=24:duration=1','-c:v','mpeg4','-q:v','3',str(silent)])
    base=[cli,'encode',str(src),str(out),'--python',py,'--script',script,'--ffmpeg',ffmpeg,'--ffprobe',ffprobe,'--tvc-encoder',tvc,'--tnv3-tool',tnv3,'--qp','30','--jobs','1','--profile','pocket-sidecar']
    ok=subprocess.run(base,check=True,text=True,capture_output=True,env=env); print('PAIR_CLI_STDOUT:',ok.stdout.strip()); print('PAIR_CLI_STDERR:',ok.stderr.strip())
    audio=out.with_suffix('.mp3')
    checked=run([tnv3,'check',str(out)]); info=run([tnv3,'info',str(out)]).stdout.strip()
    print('TNV3_CHECK:',checked.stdout.strip() or 'exit 0'); print('TNV3_INFO:',info)
    video_json=json.loads(run([ffprobe,'-v','error','-count_frames','-select_streams','v:0','-show_entries','stream=codec_name,width,height,nb_read_frames','-of','json',str(src)]).stdout)
    demux=d/'demux.tvc'; y4m=d/'decoded.y4m'
    run([tnv3,'demux',str(out),str(demux)])
    run([tvc_dec,str(demux),str(y4m)])
    decoded_json=json.loads(run([ffprobe,'-v','error','-count_frames','-show_entries','stream=codec_name,width,height,nb_read_frames','-of','json',str(y4m)]).stdout)
    aud_json=json.loads(run([ffprobe,'-v','error','-select_streams','a:0','-show_entries','stream=codec_name,sample_rate,channels,duration','-of','json',str(audio)]).stdout)
    print('SOURCE_VIDEO_STREAM:',video_json['streams'][0]); print('DECODED_TNV3_Y4M_STREAM:',decoded_json['streams'][0]); print('MP3_STREAM:',aud_json['streams'][0]); print('TNV3_DECODE:', 'tnv3 demux + tvc_dec + ffprobe succeeded')
    assert int(decoded_json['streams'][0]['nb_read_frames'])==24
    assert int(aud_json['streams'][0]['sample_rate'])==48000 and int(aud_json['streams'][0]['channels'])==2
    assert .95 <= float(aud_json['streams'][0]['duration']) <= 1.15
    # no-audio fails and publishes neither output
    noout=d/'silent.tnv3'; noargs=base.copy(); noargs[2]=str(silent); noargs[3]=str(noout)
    failed=subprocess.run(noargs,text=True,capture_output=True,env=env)
    print('NO_AUDIO_EXIT:',failed.returncode,'STDERR:',failed.stderr.strip())
    assert failed.returncode!=0 and not noout.exists() and not noout.with_suffix('.mp3').exists()
    assert not list(d.glob('.tau-publish-*')), 'failed encode left a staging directory behind'
    # Existing TNV3 blocks before encode and remains exact.
    keep=d/'existing.tnv3'; keep.write_bytes(b'KEEP_VIDEO'); args=base.copy(); args[3]=str(keep)
    failv=subprocess.run(args,text=True,capture_output=True,env=env)
    assert failv.returncode!=0 and keep.read_bytes()==b'KEEP_VIDEO' and not keep.with_suffix('.mp3').exists()
    # Existing MP3 blocks before encode and remains exact.
    audio_base=d/'audiooccupied.tnv3'; keepmp=audio_base.with_suffix('.mp3'); keepmp.write_bytes(b'KEEP_AUDIO'); args=base.copy(); args[3]=str(audio_base)
    faila=subprocess.run(args,text=True,capture_output=True,env=env)
    print('PREEXISTING_OUTPUT_ERRORS:',failv.stderr.strip(), '|',faila.stderr.strip())
    assert faila.returncode!=0 and keepmp.read_bytes()==b'KEEP_AUDIO' and not audio_base.exists()
    print('PASS: pair encode, checker, decode/stream probes, no-audio cleanup, and no-clobber preservation')

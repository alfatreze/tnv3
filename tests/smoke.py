#!/usr/bin/env python3
"""Synthetic reproducible encoder/decoder/container smoke check."""
import hashlib, os, pathlib, subprocess, tempfile, wave
root=pathlib.Path(__file__).resolve().parents[1]
def run(*args, env=None):
 p=subprocess.run([str(a) for a in args],capture_output=True,text=True,env=env)
 if p.returncode:raise RuntimeError(f'{args}: {p.stderr}')
 return p
with tempfile.TemporaryDirectory() as td:
 td=pathlib.Path(td);src=td/'input.y4m';wav=td/'silence.wav';tac=td/'audio.tac';out=[];w,h,n=400,176,24
 with src.open('wb') as f:
  f.write(f'YUV4MPEG2 W{w} H{h} F24:1 Ip C420jpeg\n'.encode())
  for t in range(n):
   f.write(b'FRAME\n');f.write(bytes(16+((x//5+y//4+t*3)%220) for y in range(h) for x in range(w)));f.write(bytes([128])*(w*h//2))
 with wave.open(str(wav),'wb') as f:f.setnchannels(2);f.setsampwidth(2);f.setframerate(48000);f.writeframes(bytes(48000*2*2))
 run(root/'tools/tac_ref/tac','enc',wav,tac,'-b','96')
 decoded_audio=td/'decoded.wav';fixed_audio=td/'fixed.wav'
 run(root/'tools/tac_ref/tac','dec',tac,decoded_audio)
 run(root/'tools/tac_ref/tac_fx','test')
 run(root/'tools/tac_ref/tac_fx','dec',tac,fixed_audio)
 for audio in (decoded_audio,fixed_audio):
  with wave.open(str(audio),'rb') as f:
   if (f.getnchannels(),f.getframerate(),f.getsampwidth(),f.getnframes())!=(2,48000,2,48000):raise AssertionError('audio format or sample count mismatch')
 for name in ['tvc_enc','tvc_enc_slice_experimental']:
  stream=td/(name+'.tvc');rec=td/(name+'.y4m');dec=td/(name+'-dec.y4m');tnv=td/(name+'.tnv3')
  run(root/'tools/tvc_ref'/name,src,stream,'-q','30','-n',str(n),'-l','3','-r',rec)
  run(root/'tools/tvc_ref/tvc_dec',stream,dec)
  if rec.read_bytes()!=dec.read_bytes():raise AssertionError(name+' reconstruction mismatch')
  run(root/'tools/tnv3_ref/tnv3','mux',stream,'-a',tac,'-T','-c','-o',tnv);run(root/'tools/tnv3_ref/tnv3','check',tnv)
  demux_video=td/(name+'-demux.tvc');demux_audio=td/(name+'-demux.tac');demux_wav=td/(name+'-demux.wav')
  run(root/'tools/tnv3_ref/tnv3','demux',tnv,demux_video,demux_audio)
  if stream.read_bytes()!=demux_video.read_bytes():raise AssertionError('container video round-trip mismatch')
  run(root/'tools/tac_ref/tac','dec',demux_audio,demux_wav)
  with wave.open(str(demux_wav),'rb') as f:
   if (f.getnchannels(),f.getframerate(),f.getsampwidth(),f.getnframes())!=(2,48000,2,48000):raise AssertionError('container audio sample count mismatch')
  out.append((hashlib.sha256(stream.read_bytes()).hexdigest(),hashlib.sha256(tnv.read_bytes()).hexdigest()))
 if out[0]!=out[1]:raise AssertionError('serial and slice output differ')
 for key,value in [('TVC_B_WORKERS','1'),('TVC_BPOOL_TEST_INIT_FAIL','1')]:
  env=os.environ.copy();env[key]=value;stream=td/(key+'.tvc');rec=td/(key+'.y4m')
  run(root/'tools/tvc_ref/tvc_enc_slice_experimental',src,stream,'-q','30','-n',str(n),'-l','3','-r',rec,env=env)
  if hashlib.sha256(stream.read_bytes()).hexdigest()!=out[0][0]:raise AssertionError(key+' fallback differs')
 print('PASS serial/slice TVC reconstruction, TAC audio and TNV3 check',out[0][1])

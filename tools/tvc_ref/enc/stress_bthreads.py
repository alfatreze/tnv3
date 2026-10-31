#!/usr/bin/env python3
"""Compare the isolated B-worker prototype with production, including level retries."""
import os,pathlib,subprocess,hashlib,json,re,tempfile
root=pathlib.Path(__file__).resolve().parents[3];work=root/'work/encoder_lab'
candidate=pathlib.Path(os.environ.get('TVC_BTHREADS_BINARY',str(work/'encoder-bthreads'))).resolve()
serial=pathlib.Path(os.environ.get('TVC_SERIAL_BINARY',str(root/'tools/tvc_ref/tvc_enc'))).resolve()
if not candidate.is_relative_to(root) or not serial.is_relative_to(root):raise ValueError('encoders must be inside the worktree')
cases=[('action_400',['-q','8','-n','48','-l','1']),('anime_dialog_400',['-q','12','-n','36','-l','1','-b','3','-a','1']),('bw_room_800',['-q','24','-n','24','-l','3']),('action_400',['-q','30','-n','24','-l','1','-b','1']),('action_400',['-q','20','-n','36','-l','1','-b','3','-a','1','-p','0','-x','0','-S','3'])]
rows=[]
for clip,args in cases:
 with tempfile.TemporaryDirectory(prefix='bworker-check-',dir=work) as td:
  td=pathlib.Path(td);streams=[];recons=[];diagnostics=[]
  for label,binary in [('serial',serial),('workers',candidate)]:
   stream=td/(label+'.tvc');rec=td/(label+'.y4m');dec=td/(label+'-decoded.y4m')
   p=subprocess.run([str(binary),str(work/'ref'/(clip+'.y4m')),str(stream),*args,'-r',str(rec)],capture_output=True,text=True,check=True)
   subprocess.run([str(root/'tools/tvc_ref/tvc_dec'),str(stream),str(dec)],capture_output=True,check=True)
   assert rec.read_bytes()==dec.read_bytes()
   streams.append(hashlib.sha256(stream.read_bytes()).hexdigest());recons.append(hashlib.sha256(rec.read_bytes()).hexdigest());diagnostics.append(p.stderr)
  assert len(set(streams))==len(set(recons))==1
  row=dict(clip=clip,args=args,stream_sha256=streams[0],recon_sha256=recons[0],diagnostics=diagnostics,match=True);rows.append(row);print(clip,args,'PASS',flush=True)
pathlib.Path(os.environ.get('TVC_STRESS_OUTPUT',str(root/'docs/encoder_bthreads_stress.json'))).write_text(json.dumps(rows,indent=2)+'\n')
assert any(int(m.group(1))>0 for r in rows for d in r['diagnostics'] for m in re.finditer(r'(\d+) pictures re-encoded for the level',d)), 'rate-retry path not exercised'

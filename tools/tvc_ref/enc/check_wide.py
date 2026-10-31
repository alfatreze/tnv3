#!/usr/bin/env python3
"""Compare four-worker B groups with serial, including prepared-job failures."""
import hashlib,json,os,pathlib,re,subprocess,tempfile
root=pathlib.Path(__file__).resolve().parents[3];work=root/'work/encoder_lab';rows=[]
cases=[('action_400',['-q','8','-n','24','-l','1','-b','4']),('action_400',['-q','8','-n','24','-l','1','-b','6']),('bw_room_800',['-q','24','-n','24','-l','3','-b','6'])]
for clip,args in cases:
 with tempfile.TemporaryDirectory(dir=work) as d:
  d=pathlib.Path(d); results=[]
  for name,binary in [('serial',root/'tools/tvc_ref/tvc_enc'),('wide',work/'encoder-bwide')]:
   stream=d/(name+'.tvc');rec=d/(name+'.y4m');dec=d/(name+'-dec.y4m')
   p=subprocess.run([str(binary),str(work/'ref'/f'{clip}.y4m'),str(stream),*args,'-r',str(rec)],capture_output=True,text=True,check=True)
   subprocess.run([str(root/'tools/tvc_ref/tvc_dec'),str(stream),str(dec)],capture_output=True,check=True)
   assert rec.read_bytes()==dec.read_bytes()
   results.append(dict(name=name,stream=hashlib.sha256(stream.read_bytes()).hexdigest(),recon=hashlib.sha256(rec.read_bytes()).hexdigest(),diagnostics=p.stderr))
  assert results[0]['stream']==results[1]['stream'] and results[0]['recon']==results[1]['recon']
  rows.append(dict(clip=clip,args=args,results=results));print(clip,args,'PASS',flush=True)
assert any(int(m.group(1))>0 for r in rows for x in r['results'] for m in re.finditer(r'(\d+) pictures re-encoded for the level',x['diagnostics']))
with tempfile.TemporaryDirectory(dir=work) as d:
 d=pathlib.Path(d);hashes=[]
 for pos in range(1,5):
  env=os.environ.copy();env['TVC_BPOOL_TEST_PREP_FAIL_AT']=str(pos);stream=d/f'fail{pos}.tvc';rec=d/f'fail{pos}.y4m'
  subprocess.run([str(work/'encoder-bwide'),str(work/'ref/action_400.y4m'),str(stream),'-q','30','-n','24','-l','1','-b','6','-a','1','-r',str(rec)],env=env,capture_output=True,check=True)
  hashes.append(dict(pos=pos,stream=hashlib.sha256(stream.read_bytes()).hexdigest(),recon=hashlib.sha256(rec.read_bytes()).hexdigest()))
 stream=d/'serial.tvc';rec=d/'serial.y4m'
 subprocess.run([str(root/'tools/tvc_ref/tvc_enc'),str(work/'ref/action_400.y4m'),str(stream),'-q','30','-n','24','-l','1','-b','6','-a','1','-r',str(rec)],capture_output=True,check=True)
 assert all(x['stream']==hashlib.sha256(stream.read_bytes()).hexdigest() and x['recon']==hashlib.sha256(rec.read_bytes()).hexdigest() for x in hashes)
(root/'docs/encoder_bwide_stress.json').write_text(json.dumps(dict(cases=rows,preparation_failures=hashes),indent=2)+'\n')
print('all preparation failures PASS',flush=True)

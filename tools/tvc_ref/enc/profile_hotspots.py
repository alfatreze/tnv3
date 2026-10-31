#!/usr/bin/env python3
# Optional research tool: generated C and binaries stay under work/encoder_lab.
from pathlib import Path
import subprocess,json,re,argparse,hashlib
def checked_replace(text, old, new):
 if text.count(old)!=1: raise RuntimeError('profiling insertion no longer unique: '+old)
 return text.replace(old,new)

root=Path(__file__).resolve().parents[3]
(root/'work/encoder_lab').mkdir(parents=True,exist_ok=True)
ap=argparse.ArgumentParser(description='Approximate inclusive hotspot timings; inserts clocks in a temporary encoder copy. Timing-call overhead is included.')
ap.add_argument('--source',type=Path,default=root/'tools/tvc_ref/tvc_enc.c')
ap.add_argument('--out',type=Path,required=True)
opts=ap.parse_args();original=opts.source.read_text();source_hash=hashlib.sha256(original.encode()).hexdigest();s=original
dependencies={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [root/'tools/tvc_ref/tvc.h',*(root/'tools/tvc_ref/enc'/n for n in ['pixel_ops.h','bit_ops.h','qpel.h','rdoq.h'])]}
s='#include <time.h>\nstatic double prof_now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}\nstatic double pq,pr,pd; static long nq,nr,nd;\n'+s
# Time fractional prediction/SAD; the in-bounds integer fast path is untouched.
a=s.index('static int sad16(');b=s.index('static double mvbits',a);t=s[a:b]
t=checked_replace(t,'    uint8_t p[256];','    double start=prof_now();\n    uint8_t p[256];')
t=checked_replace(t,'    return enc_sad16_pixels(p, 16, sp, S.w);','    int result=enc_sad16_pixels(p, 16, sp, S.w); if(S.qpel && ((mvx|mvy)&3)){pq+=prof_now()-start;nq++;} return result;')
s=s[:a]+t+s[b:]
a=s.index('static int rdoq(');b=s.index('/* code one 8x8 residual',a);t=s[a:b]
i=t.index('{');t=t[:i+1]+' double start=prof_now();'+t[i+1:]
if '    return nz;' in t:
 t=checked_replace(t,'    return nz;','    pr+=prof_now()-start;nr++;return nz;')
else:
 t=checked_replace(t,'    return enc_rdoq(cs, lev, N, qp, cc, lam, &S.ctx);','    int result=enc_rdoq(cs, lev, N, qp, cc, lam, &S.ctx);pr+=prof_now()-start;nr++;return result;')
s=s[:a]+t+s[b:]
a=s.index('static void fdct(');b=s.index('static int quant(',a);t=s[a:b];i=t.index('{');t=t[:i+1]+' double start=prof_now();'+t[i+1:];i=t.rfind('}');t=t[:i]+'pd+=prof_now()-start;nd++;'+t[i:];s=s[:a]+t+s[b:]
s=checked_replace(s,'int main(int argc, char **argv) {','int main(int argc, char **argv) { double total_start=prof_now();')
s=checked_replace(s,'    fprintf(stderr, "%d frames,','    fprintf(stderr,"HOT %.9f %.9f %.9f %.9f %ld %ld %ld\\n",prof_now()-total_start,pq,pr,pd,nq,nr,nd);\n    fprintf(stderr, "%d frames,')
p=root/'work/encoder_lab/hotspots.c';p.write_text(s)
subprocess.run(['cc','-O3','-I',str(root/'tools/tvc_ref'),'-o',str(root/'work/encoder_lab/hotspots'),str(p),'-lm'],check=True)
rows=[]
for case in ['action_400','anime_dialog_400','action_800']:
 r=subprocess.run([str(root/'work/encoder_lab/hotspots'),str(root/'work/encoder_lab/ref'/f'{case}.y4m'),str(root/'work/encoder_lab/hotspots.tvc'),'-q','30','-n','96'],capture_output=True,text=True,check=True)
 m=re.search(r'HOT ([\d.]+) ([\d.]+) ([\d.]+) ([\d.]+) (\d+) (\d+) (\d+)',r.stderr);assert m,r.stderr
 total,q,rdoq,dct=map(float,m.groups()[:4]);nq,nr,nd=map(int,m.groups()[4:]);row=dict(source_sha256=source_hash,dependency_sha256=dependencies,instrumentation='inclusive clock timings; timing-call overhead included; fdct8 excludes 4x4 transforms',case=case,qp=30,frames=96,instrumented_total_s=total,fractional_search_sad_s=q,rdoq_s=rdoq,fdct8_s=dct,fractional_search_calls=nq,rdoq_calls=nr,fdct8_calls=nd)
 rows.append(row);print(row,flush=True)
opts.out.parent.mkdir(parents=True,exist_ok=True)
opts.out.write_text(json.dumps(rows,indent=2)+'\n')

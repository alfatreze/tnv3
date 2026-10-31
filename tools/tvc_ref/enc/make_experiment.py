#!/usr/bin/env python3
"""Build isolated native research encoder variants under work/; production C is unchanged."""
import argparse,pathlib,subprocess,hashlib,json
root=pathlib.Path(__file__).resolve().parents[3]
ap=argparse.ArgumentParser();ap.add_argument('variant',choices=['vt','bthreads','bpool','bcontext','bargs','bserialctx','bsafe','bmotion','bslice','blook','bwide']);o=ap.parse_args()
s=(root/'tools/tvc_ref/tvc_enc.c').read_text();original=hashlib.sha256(s.encode()).hexdigest()
def swap(old,new):
 global s
 if s.count(old)!=1:raise RuntimeError('insertion drift: '+old[:100])
 s=s.replace(old,new)
if o.variant=='vt':
 swap('#include "enc/rdoq.h"','#include "enc/rdoq.h"\nextern int enc_vt_seeds(const uint8_t*,const uint8_t*,int,int,int,int,int*);\nstatic int *VT_MVS[2];')
 swap('int cand[5][2] =', 'int cand[6][2] =')
 swap('    for (int i = 0; i < 5; i++) {','    int ncand=5; if(VT_MVS[L]) {cand[5][0]=VT_MVS[L][(y*S.mbw+x)*2];cand[5][1]=VT_MVS[L][(y*S.mbw+x)*2+1];ncand=6;}\n    for (int i = 0; i < ncand; i++) {')
 swap('    S.ptype = ptype; S.qp = qp; S.aq = aq_on;', '''    for(int l=0;l<2;l++){free(VT_MVS[l]);VT_MVS[l]=NULL;}
    if(ptype!=PIC_I)for(int l=0;l<(ptype==PIC_B?2:1);l++){
        int *v=malloc((size_t)S.mbw*S.mbh*2*sizeof(int));
        if(enc_vt_seeds(REFS[l]->p[0],SRC.p[0],S.w,S.w,S.h,S.qpel?4:2,v))VT_MVS[l]=v;else free(v);
    }
    S.ptype = ptype; S.qp = qp; S.aq = aq_on;''')
else:
 swap('#include "enc/rdoq.h"','#include "enc/rdoq.h"\n#include <pthread.h>')
 swap('static Pic S; static Plane3 SRC, CUR;', 'static _Thread_local Pic S; static _Thread_local Plane3 SRC, CUR;')
 swap('static const Plane3 *REFS[2];','static _Thread_local const Plane3 *REFS[2];')
 swap('static int16_t lev[6][64], blev[6][64], tlev[6][64];','static _Thread_local int16_t lev[6][64], blev[6][64], tlev[6][64];')
 swap('int main(int argc, char **argv) {', '''typedef struct {Pic pic;Plane3 src,cur;const Plane3 *r0,*r1;double *off;int qp,aq;uint8_t *data;size_t len;} BJob;
static void *bwork(void *arg){BJob *j=arg;S=j->pic;S.mb=calloc((size_t)S.mbw*S.mbh,sizeof(MB));if(!S.mb)abort();SRC=j->src;CUR=j->cur;REFS[0]=j->r0;REFS[1]=j->r1;j->len=encode_picture_buf(&j->data,PIC_B,j->qp,j->off,j->aq);free(S.mb);return NULL;}
int main(int argc, char **argv) {''')
 start=s.index('        /* the B pictures between the two anchors */');end=s.index('        last_t = t;',start)
 old=s[start:end]
 new='''        /* Isolated prototype: join B workers before any anchor/reference reuse.
         * Parent commits rate budgets in order; over-budget jobs use serial retry. */
        /* A caller running segment processes may reserve one CPU per process
         * by setting TVC_B_WORKERS=1.  The default remains the measured pair. */
        const char *worker_limit=getenv("TVC_B_WORKERS");
        if(last_t>=0 && t-last_t>2 && !(worker_limit && !strcmp(worker_limit,"1"))){
            for(int first=last_t+1;first<t;first+=2){
                BJob jobs[2];pthread_t threads[2];int count=t-first<2?t-first:2;
                for(int k=0;k<count;k++){int b=first+k;BJob *j=&jobs[k];memset(j,0,sizeof *j);j->pic=S;j->src=src[b];j->cur=plane_alloc(S.w,S.h);if(o.recon)rec[b]=j->cur;j->r0=prev;j->r1=last;j->qp=tvc_clip(o.qp+o.boff,0,51);j->aq=aq_on;j->off=calloc(nm,sizeof(double));if(o.aq>0)aq_offsets(&src[b],o.aq,j->off);if(pthread_create(&threads[k],NULL,bwork,j))abort();}
                for(int k=0;k<count;k++){if(pthread_join(threads[k],NULL))abort();BJob *j=&jobs[k];long allow=LEV?(RATEWIN?window_allowance():LEV->maxpic):0;if(LEV&&(long)j->len>allow){SRC=j->src;CUR=j->cur;REFS[0]=j->r0;REFS[1]=j->r1;encode_picture(out,&r,PIC_B,j->qp,j->off,j->aq,&total);}else{HIST[NHIST++]=(long)j->len;put32(out,(uint32_t)j->len);fwrite(j->data,1,j->len,out);total+=j->len+4;}free(j->data);free(j->off);if(!o.recon)plane_free(&j->cur);}
            }
        }else{
'''+old+'        }\n'
 s=s[:start]+new+s[end:]
 if o.variant in ('bpool','bcontext','bargs','bserialctx','bsafe','bmotion','bslice','blook','bwide'):
  swap('#include <pthread.h>', '#include "enc/bpool.h"')
  swap('int main(int argc, char **argv) {', 'static void bpool_work(void *arg){(void)bwork(arg);}\nint main(int argc, char **argv) {')
  swap('    size_t total = 0; RC r;', '    const char *pool_limit=getenv("TVC_B_WORKERS");\n    EncPool pool; int pool_ok = !(pool_limit && !strcmp(pool_limit,"1")) && !getenv("TVC_BPOOL_TEST_INIT_FAIL") && enc_pool_init(&pool);\n    size_t total = 0; RC r;')
  swap('if(last_t>=0 && t-last_t>2 && !(worker_limit && !strcmp(worker_limit,"1")))', 'if(pool_ok && last_t>=0 && t-last_t>2 && !(worker_limit && !strcmp(worker_limit,"1")))')
  swap('BJob jobs[2];pthread_t threads[2];int count=', 'BJob jobs[2];int count=')
  swap('if(pthread_create(&threads[k],NULL,bwork,j))abort();', 'enc_pool_submit(&pool,k,bpool_work,j);')
  swap('if(pthread_join(threads[k],NULL))abort();', 'enc_pool_wait(&pool,k);')
  swap('if(!S.mb)abort();', 'if(!S.mb){j->data=NULL;j->len=0;return NULL;}')
  swap('if(LEV&&(long)j->len>allow)', 'if(!j->data || (LEV&&(long)j->len>allow))')
  swap('    fclose(out);', '    if(pool_ok) enc_pool_destroy(&pool);\n    fclose(out);')
 if o.variant in ('bcontext','bargs','bserialctx','bsafe','bmotion','bslice','blook','bwide'):
  swap('static _Thread_local Pic S; static _Thread_local Plane3 SRC, CUR;\nstatic _Thread_local const Plane3 *REFS[2];', '''typedef struct { Pic pic; Plane3 src, cur; const Plane3 *refs[2];
    int16_t lev[6][64], blev[6][64], tlev[6][64]; } EncPictureContext;
static _Thread_local EncPictureContext *enc_ctx;
#define S (enc_ctx->pic)
#define SRC (enc_ctx->src)
#define CUR (enc_ctx->cur)
#define REFS (enc_ctx->refs)''')
  swap('    static _Thread_local int16_t lev[6][64], blev[6][64], tlev[6][64];', '    int16_t (*lev)[64]=enc_ctx->lev, (*blev)[64]=enc_ctx->blev, (*tlev)[64]=enc_ctx->tlev;')
  start=s.index('static size_t encode_picture_buf(');end=s.index('/* Largest length',start);piece=s[start:end]
  for name in ('lev','blev','tlev'):piece=piece.replace('sizeof '+name, 'sizeof enc_ctx->'+name)
  s=s[:start]+piece+s[end:]
  swap('uint8_t *data;size_t len;} BJob;', 'uint8_t *data;size_t len;EncPictureContext context;} BJob;')
  swap('static void *bwork(void *arg){BJob *j=arg;S=', 'static void *bwork(void *arg){BJob *j=arg;enc_ctx=&j->context;S=')
  swap('int main(int argc, char **argv) {', 'int main(int argc, char **argv) {\n    EncPictureContext main_context = {0}; enc_ctx = &main_context;')
  if o.variant in ('bargs','bserialctx','bsafe','bmotion','bslice','blook','bwide'):
   from pass_context import convert_context
   s, context_functions=convert_context(s)
   print('explicit context:', ', '.join(context_functions))
   if o.variant in ('bsafe','bwide'):
    swap('S.mb=calloc((size_t)S.mbw*S.mbh,sizeof(MB));', 'S.mb=getenv("TVC_BPOOL_TEST_MB_FAIL")?NULL:calloc((size_t)S.mbw*S.mbh,sizeof(MB));')
    start=s.index('                BJob jobs[2];int count=');end=s.index('                for(int k=0;k<count;k++){enc_pool_wait',start)
    s=s[:start]+'''                BJob jobs[2] = {0}; int count=t-first<2?t-first:2, ready=1, made=0;
                const char *fail_at=getenv("TVC_BPOOL_TEST_PREP_FAIL_AT");
                for(int k=0;k<count;k++){
                    int b=first+k; BJob *j=&jobs[k]; j->pic=S; j->src=src[b];
                    j->cur=plane_alloc(S.w,S.h); j->off=calloc(nm,sizeof(double)); made++;
                    if((fail_at && atoi(fail_at)==k+1) || !j->cur.p[0] || !j->cur.p[1] || !j->cur.p[2] || !j->off){ready=0;break;}
                    j->r0=prev;j->r1=last;j->qp=tvc_clip(o.qp+o.boff,0,51);j->aq=aq_on;
                    if(o.aq>0)aq_offsets(enc_ctx, &src[b],o.aq,j->off);
                }
                if(!ready){
                    for(int k=0;k<made;k++){plane_free(&jobs[k].cur);free(jobs[k].off);}
                    for(int b=first;b<first+count;b++){
                        double *z=calloc(nm,sizeof *z);if(o.aq>0)aq_offsets(enc_ctx,&src[b],o.aq,z);
                        SRC=src[b];if(o.recon){rec[b]=plane_alloc(S.w,S.h);CUR=rec[b];}else CUR=bbuf;
                        REFS[0]=prev;REFS[1]=last;
                        encode_picture(enc_ctx,out,&r,PIC_B,tvc_clip(o.qp+o.boff,0,51),z,aq_on,&total);free(z);
                    }
                    continue;
                }
                for(int k=0;k<count;k++){if(o.recon)rec[first+k]=jobs[k].cur;enc_pool_submit(&pool,k,bpool_work,&jobs[k]);}
''' + s[end:]
    if o.variant=='bwide':
     swap('&& enc_pool_init(&pool);','&& enc_pool_init_n(&pool,4);')
     swap('!(pool_limit && !strcmp(pool_limit,"1"))', '(!pool_limit || !strcmp(pool_limit,"4"))')
     swap('for(int first=last_t+1;first<t;first+=2)', 'for(int first=last_t+1;first<t;first+=4)')
     swap('BJob jobs[2] = {0}; int count=t-first<2?t-first:2, ready=1, made=0;', 'BJob jobs[4] = {0}; int count=t-first<4?t-first:4, ready=1, made=0;')
   if o.variant in ('bserialctx','bmotion','bslice','blook'):
    swap('#include "enc/bpool.h"\n','')
    start=s.index('typedef struct {Pic pic;Plane3 src,cur;');end=s.index('int main(int argc, char **argv) {',start);s=s[:start]+s[end:]
    start=s.index('    const char *pool_limit=getenv("TVC_B_WORKERS");');end=s.index('    size_t total = 0; RC r;',start);s=s[:start]+s[end:]
    start=s.index('        /* Isolated prototype: join B workers');serial=s.index('        /* the B pictures between the two anchors */',start);end=s.index('        last_t = t;',serial)
    piece=s[serial:end]
    if not piece.endswith('        }\n'):raise RuntimeError('serial block drift')
    s=s[:start]+piece[:-10]+s[end:]
    swap('    if(pool_ok) enc_pool_destroy(&pool);\n','')
    if o.variant=='bmotion':
     swap('#include "enc/rdoq.h"', '#include "enc/rdoq.h"\n#include "enc/bpool.h"')
     swap('/* encode one picture into a malloc\'d buffer', '''typedef struct { EncPictureContext *ctx; const Plane3 *ref, *src; int x,y,l,px,py; double ls; int *mx,*my; } MotionTask;
static EncPool motion_pool; static int motion_pool_ok;
static void motion_task(void *arg) { MotionTask *j=arg; motion_search(j->ctx,j->ref,j->src,j->x,j->y,j->l,j->px,j->py,j->ls,1,j->mx,j->my); }
/* encode one picture into a malloc'd buffer''')
     swap('            for (int l = 0; l < nl; l++) motion_search(enc_ctx, REFS[l], &SRC, x, y, l, pv[l][0], pv[l][1], ls, 1, &mv[l][0], &mv[l][1]);', '''            if(nl==2 && motion_pool_ok){
                MotionTask job={enc_ctx,REFS[1],&SRC,x,y,1,pv[1][0],pv[1][1],ls,&mv[1][0],&mv[1][1]};
                enc_pool_submit(&motion_pool,0,motion_task,&job);
                motion_search(enc_ctx,REFS[0],&SRC,x,y,0,pv[0][0],pv[0][1],ls,1,&mv[0][0],&mv[0][1]);
                enc_pool_wait(&motion_pool,0);
            }else for (int l = 0; l < nl; l++) motion_search(enc_ctx, REFS[l], &SRC, x, y, l, pv[l][0], pv[l][1], ls, 1, &mv[l][0], &mv[l][1]);''')
     swap('    size_t total = 0; RC r;', '    motion_pool_ok=enc_pool_init(&motion_pool);\n    size_t total = 0; RC r;')
     swap('    fclose(out);', '    if(motion_pool_ok)enc_pool_destroy(&motion_pool);\n    fclose(out);')
    if o.variant=='bslice':
     swap('#include "enc/rdoq.h"', '#include "enc/rdoq.h"\n#include "enc/bpool.h"')
     fn=s.index('static size_t encode_picture_buf(');start=s.index('    for (int k = 0; k < ns; k++) {',fn);end=s.index('    S.slice_y0 = 0;',start)
     loop=s[start:end];first=loop.index('\n')+1
     if not loop.endswith('    }\n'):raise RuntimeError('slice loop drift')
     body=loop[first:-6]
     helper='''typedef struct { EncPictureContext ctx; RC *rs; int k, rows, qp, ptype, aq_on; const double *off; } SliceTask;
static EncPool slice_pool; static int slice_pool_ok;
static void encode_slice_task(void *arg) {
    SliceTask *j=arg; EncPictureContext *enc_ctx=&j->ctx; RC *rs=j->rs;
    int k=j->k, rows=j->rows, qp=j->qp, ptype=j->ptype, aq_on=j->aq_on; const double *off=j->off;
    int16_t (*lev)[64]=enc_ctx->lev, (*blev)[64]=enc_ctx->blev, (*tlev)[64]=enc_ctx->tlev;
'''+body+'}\n'
     parallel='''    if(slice_pool_ok && ns>1){
        for(int first=0;first<ns;first+=2){
            int count=ns-first<2?ns-first:2; SliceTask jobs[2];
            for(int j=0;j<count;j++){
                jobs[j]=(SliceTask){.ctx=*enc_ctx,.rs=rs,.k=first+j,.rows=rows,.qp=qp,.ptype=ptype,.aq_on=aq_on,.off=off};
                enc_pool_submit(&slice_pool,j,encode_slice_task,&jobs[j]);
            }
            for(int j=0;j<count;j++)enc_pool_wait(&slice_pool,j);
        }
    }else{\n'''+loop+'    }\n'
     s=s[:start]+parallel+s[end:];fn=s.index('static size_t encode_picture_buf(');s=s[:fn]+helper+s[fn:]
     swap('    size_t total = 0; RC r;', '    slice_pool_ok=enc_pool_init(&slice_pool);\n    size_t total = 0; RC r;')
     swap('    fclose(out);', '    if(slice_pool_ok)enc_pool_destroy(&slice_pool);\n    fclose(out);')
    if o.variant=='blook':
     swap('#include "enc/rdoq.h"', '#include "enc/rdoq.h"\n#include "enc/bpool.h"')
     fn=s.index('static Look look_costs(');start=s.index('    for (int t = 0; t < n; t++) for (int i = 0; i < nm; i++) {',fn);end=s.index('    return L;',start)
     loop=s[start:end];header='    for (int t = 0; t < n; t++) for (int i = 0; i < nm; i++) {'
     if not loop.startswith(header) or not loop.endswith('    }\n'):raise RuntimeError('look-ahead loop drift')
     body=loop[len(header):]
     helper='''typedef struct { EncPictureContext ctx; Plane3 *src; Look *result; int begin, end; } LookTask;
static void run_look_task(void *arg) {
    LookTask *j=arg; EncPictureContext *enc_ctx=&j->ctx; Plane3 *src=j->src; Look L=*j->result;
    int nm=S.mbw*S.mbh;
    for(int t=j->begin;t<j->end;t++) for(int i=0;i<nm;i++){'''+body+'}\n'
     parallel='''    EncPool pool;
    if(n>1 && enc_pool_init(&pool)){
        LookTask jobs[2]={{.ctx=*enc_ctx,.src=src,.result=&L,.begin=0,.end=n/2},
                          {.ctx=*enc_ctx,.src=src,.result=&L,.begin=n/2,.end=n}};
        for(int k=0;k<2;k++)enc_pool_submit(&pool,k,run_look_task,&jobs[k]);
        for(int k=0;k<2;k++)enc_pool_wait(&pool,k);
        enc_pool_destroy(&pool);
    }else{\n'''+loop+'    }\n'
     s=s[:start]+parallel+s[end:];fn=s.index('static Look look_costs(');s=s[:fn]+helper+s[fn:]
work=root/'work/encoder_lab';work.mkdir(parents=True,exist_ok=True)
source=work/f'experiment-{o.variant}.c';binary=work/f'encoder-{o.variant}';source.write_text(s)
cmd=['cc','-O3','-I',str(root/'tools/tvc_ref'),str(source)]
if o.variant=='vt':
 obj=work/'vt_seeds.o'
 subprocess.run(['clang','-O3','-fblocks','-fobjc-arc','-c',str(root/'tools/tvc_ref/enc/vt_seeds.m'),'-o',str(obj)],check=True)
 cmd += [str(obj),'-framework','Foundation','-framework','VideoToolbox','-framework','CoreVideo','-framework','CoreMedia']
elif o.variant!='bserialctx':cmd+=['-pthread']
cmd+=['-lm','-o',str(binary)];subprocess.run(cmd,check=True)
(work/f'experiment-{o.variant}.json').write_text(json.dumps(dict(variant=o.variant,base_source_sha256=original,generated_source_sha256=hashlib.sha256(s.encode()).hexdigest(),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),command=cmd),indent=2)+'\n')
print(binary)

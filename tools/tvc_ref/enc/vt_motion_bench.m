/* Optional macOS26 research program; not linked into tvc_enc. */
#import <Foundation/Foundation.h>
#import <VideoToolbox/VideoToolbox.h>
#import <VideoToolbox/VTMotionEstimationSession.h>
#import <VideoToolbox/VTMotionEstimationSessionProperties.h>
#import <dispatch/dispatch.h>
#include <time.h>
#include <math.h>
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}
static int clip(int x,int hi){return x<0?0:x>=hi?hi-1:x;}
static int cmp(const void*a,const void*b){float x=*(const float*)a,y=*(const float*)b;return(x>y)-(x<y);}
static CVPixelBufferRef image(int w,int h,int dx,int dy,const uint8_t *pixels){
 CVPixelBufferRef p=NULL;
 NSDictionary *attrs=@{(__bridge NSString*)kCVPixelBufferIOSurfacePropertiesKey:@{},(__bridge NSString*)kCVPixelBufferBytesPerRowAlignmentKey:@64};
 if(CVPixelBufferCreate(NULL,w,h,kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,(__bridge CFDictionaryRef)attrs,&p))return NULL;
 CVPixelBufferLockBaseAddress(p,0);
 uint8_t *y=CVPixelBufferGetBaseAddressOfPlane(p,0);size_t stride=CVPixelBufferGetBytesPerRowOfPlane(p,0);
 for(int j=0;j<h;j++)for(int i=0;i<w;i++)y[j*stride+i]=pixels[clip(j-dy,h)*w+clip(i-dx,w)];
 uint8_t *uv=CVPixelBufferGetBaseAddressOfPlane(p,1);memset(uv,128,CVPixelBufferGetBytesPerRowOfPlane(p,1)*CVPixelBufferGetHeightOfPlane(p,1));
 CVPixelBufferUnlockBaseAddress(p,0);return p;
}
int main(int argc,char**argv){@autoreleasepool{
 if(@available(macOS 26.0,*)){
 int w=argc>1?atoi(argv[1]):400,h=argc>2?atoi(argv[2]):176,multipass=argc>3?atoi(argv[3]):0,smooth=argc>4?atoi(argv[4]):0;
 if(w<64||h<64||w%16||h%16)return 2;
 uint8_t *pixels=malloc((size_t)w*h);uint32_t seed=17;
 for(int i=0;i<w*h;i++){seed=seed*1664525u+1013904223u;pixels[i]=16+(seed>>24)%220;}
 if(smooth){uint8_t *noise=malloc((size_t)w*h);memcpy(noise,pixels,(size_t)w*h);for(int y=0;y<h;y++)for(int x=0;x<w;x++){int gx=x/8,gy=y/8,fx=x%8,fy=y%8;pixels[y*w+x]=(noise[gy*w+gx]*(8-fx)*(8-fy)+noise[gy*w+gx+1]*fx*(8-fy)+noise[(gy+1)*w+gx]*(8-fx)*fy+noise[(gy+1)*w+gx+1]*fx*fy+32)>>6;}free(noise);}
 CVPixelBufferRef ref=image(w,h,0,0,pixels);if(!ref)return 3;
 VTMotionEstimationSessionRef s=NULL;OSStatus status=VTMotionEstimationSessionCreate(NULL,(__bridge CFDictionaryRef)@{(__bridge NSString*)kVTMotionEstimationSessionCreationOption_MotionVectorSize:@16,(__bridge NSString*)kVTMotionEstimationSessionCreationOption_UseMultiPassSearch:@(multipass!=0)},w,h,&s);
 if(status){fprintf(stderr,"session create %d\n",(int)status);return 4;}
 NSMutableArray *rows=[NSMutableArray new];int shifts[][2]={{0,0},{4,0},{-4,0},{0,4},{4,-2},{16,0}};
 for(int k=0;k<6;k++){
  CVPixelBufferRef cur=image(w,h,shifts[k][0],shifts[k][1],pixels);if(!cur)return 3;
  NSMutableArray *times=[NSMutableArray new];NSMutableDictionary *row=[@{@"dx":@(shifts[k][0]),@"dy":@(shifts[k][1])} mutableCopy];
  for(int trial=-2;trial<8;trial++){
   __block CVPixelBufferRef vectors=NULL;__block OSStatus result=-1;
   dispatch_semaphore_t sem=dispatch_semaphore_create(0);double t=now();
   OSStatus submit=VTMotionEstimationSessionEstimateMotionVectors(s,ref,cur,0,NULL,^(OSStatus st,VTMotionEstimationInfoFlags f,CFDictionaryRef info,CVPixelBufferRef v){result=st;if(v)vectors=CVPixelBufferRetain(v);dispatch_semaphore_signal(sem);});
   if(submit){fprintf(stderr,"submit %d\n",(int)submit);return 5;}
   if(dispatch_semaphore_wait(sem,dispatch_time(DISPATCH_TIME_NOW,30*NSEC_PER_SEC))){fputs("motion timeout\n",stderr);exit(6);}
   double ms=(now()-t)*1000;if(result||!vectors){fprintf(stderr,"callback %d\n",(int)result);return 7;}
   if(trial>=0)[times addObject:@(ms)];
   if(trial==7){
    OSType fmt=CVPixelBufferGetPixelFormatType(vectors);int ow=(int)CVPixelBufferGetWidth(vectors),oh=(int)CVPixelBufferGetHeight(vectors);size_t stride=CVPixelBufferGetBytesPerRow(vectors);
    char fourcc[5]={(fmt>>24)&255,(fmt>>16)&255,(fmt>>8)&255,fmt&255,0};
    row[@"format"]=@(fourcc);row[@"output_width"]=@(ow);row[@"output_height"]=@(oh);row[@"stride"]=@(stride);
    CVPixelBufferLockBaseAddress(vectors,kCVPixelBufferLock_ReadOnly);uint8_t *base=CVPixelBufferGetBaseAddress(vectors);
    if(base){NSMutableString *hex=[NSMutableString new];for(int i=0;i<32&&i<(int)stride;i++)[hex appendFormat:@"%02x",base[i]];row[@"first_bytes"]=hex;}
    if(base&&(fmt==kCVPixelFormatType_TwoComponent16Half||fmt==kCVPixelFormatType_TwoComponent32Float)){
     float *xs=malloc((size_t)ow*oh*sizeof(float)),*ys=malloc((size_t)ow*oh*sizeof(float));int n=0;
     for(int y=2;y<oh-2;y++)for(int x=2;x<ow-2;x++){
      if(fmt==kCVPixelFormatType_TwoComponent16Half){const _Float16 *v=(const _Float16*)(base+y*stride);xs[n]=(float)v[2*x];ys[n]=(float)v[2*x+1];}
      else{const float *v=(const float*)(base+y*stride);xs[n]=v[2*x];ys[n]=v[2*x+1];}n++;
     }
     if(n){int matches=0;for(int z=0;z<n;z++)matches+=hypotf(xs[z]+shifts[k][0],ys[z]+shifts[k][1])<=1.0f;row[@"within_one_pixel_fraction"]=@((double)matches/n);qsort(xs,n,sizeof(float),cmp);qsort(ys,n,sizeof(float),cmp);row[@"median_x"]=@(xs[n/2]);row[@"median_y"]=@(ys[n/2]);row[@"interior_vectors"]=@(n);row[@"median_within_half_pixel"]=@(fabsf(xs[n/2]+shifts[k][0])<=0.5f&&fabsf(ys[n/2]+shifts[k][1])<=0.5f);}
     free(xs);free(ys);
    }
    CVPixelBufferUnlockBaseAddress(vectors,kCVPixelBufferLock_ReadOnly);
   }
   CVPixelBufferRelease(vectors);
  }
  NSArray *sorted=[times sortedArrayUsingSelector:@selector(compare:)];row[@"median_ms"]=sorted[sorted.count/2];row[@"samples_ms"]=times;[rows addObject:row];CVPixelBufferRelease(cur);
 }
 VTMotionEstimationSessionCompleteFrames(s);VTMotionEstimationSessionInvalidate(s);CFRelease(s);CVPixelBufferRelease(ref);free(pixels);
 NSDictionary *report=@{@"width":@(w),@"height":@(h),@"block_size":@16,@"multipass":@(multipass),@"smooth_fixture":@(smooth),@"input":@"deterministic limited-range texture, current(x,y)=reference(x-dx,y-dy)",@"scope":@"submit-to-callback latency; input preparation excluded; accelerator placement unverified",@"cases":rows};
 NSData *data=[NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted error:nil];if(!data)return 8;fwrite(data.bytes,1,data.length,stdout);putchar('\n');return 0;
 }else{return 2;}
}}

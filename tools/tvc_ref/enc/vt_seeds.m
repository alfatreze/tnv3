/* Optional single-threaded experimental motion-seed bridge. No production linkage. */
#import <Foundation/Foundation.h>
#import <VideoToolbox/VideoToolbox.h>
#import <VideoToolbox/VTMotionEstimationSession.h>
#import <VideoToolbox/VTMotionEstimationSessionProperties.h>
#import <dispatch/dispatch.h>
#include <math.h>
#include <time.h>
static VTMotionEstimationSessionRef session;
static int sw,sh,calls,good;
static double seconds;
static double clock_s(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}
static void cleanup(void){if(session){VTMotionEstimationSessionInvalidate(session);CFRelease(session);}fprintf(stderr,"VT_SEEDS calls=%d successful=%d ms=%.3f\n",calls,good,seconds*1000);}
static CVPixelBufferRef pixels(const uint8_t *y,int stride,int w,int h){
 CVPixelBufferRef p=NULL;NSDictionary *a=@{(__bridge NSString*)kCVPixelBufferIOSurfacePropertiesKey:@{},(__bridge NSString*)kCVPixelBufferBytesPerRowAlignmentKey:@64};
 if(CVPixelBufferCreate(NULL,w,h,kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,(__bridge CFDictionaryRef)a,&p))return NULL;
 CVPixelBufferLockBaseAddress(p,0);uint8_t *out=CVPixelBufferGetBaseAddressOfPlane(p,0);size_t os=CVPixelBufferGetBytesPerRowOfPlane(p,0);
 for(int row=0;row<h;row++)memcpy(out+row*os,y+row*stride,w);
 memset(CVPixelBufferGetBaseAddressOfPlane(p,1),128,CVPixelBufferGetBytesPerRowOfPlane(p,1)*CVPixelBufferGetHeightOfPlane(p,1));CVPixelBufferUnlockBaseAddress(p,0);return p;
}
int enc_vt_seeds(const uint8_t *ref,const uint8_t *cur,int stride,int w,int h,int units,int *xy){@autoreleasepool{
 if(@available(macOS 26.0,*)){
 double start=clock_s();calls++;
 if(!session){OSStatus st=VTMotionEstimationSessionCreate(NULL,(__bridge CFDictionaryRef)@{(__bridge NSString*)kVTMotionEstimationSessionCreationOption_MotionVectorSize:@16},w,h,&session);if(st)return 0;sw=w;sh=h;atexit(cleanup);}
 if(sw!=w||sh!=h)return 0;
 CVPixelBufferRef a=pixels(ref,stride,w,h),b=pixels(cur,stride,w,h);if(!a||!b){if(a)CVPixelBufferRelease(a);if(b)CVPixelBufferRelease(b);return 0;}
 __block CVPixelBufferRef out=NULL;__block OSStatus status=-1;dispatch_semaphore_t sem=dispatch_semaphore_create(0);
 OSStatus st=VTMotionEstimationSessionEstimateMotionVectors(session,a,b,0,NULL,^(OSStatus s,VTMotionEstimationInfoFlags f,CFDictionaryRef info,CVPixelBufferRef v){status=s;if(v)out=CVPixelBufferRetain(v);dispatch_semaphore_signal(sem);});
 if(!st&&dispatch_semaphore_wait(sem,dispatch_time(DISPATCH_TIME_NOW,30*NSEC_PER_SEC))){fputs("VT seed timeout\n",stderr);exit(3);}
 CVPixelBufferRelease(a);CVPixelBufferRelease(b);
 int ok=!st&&!status&&out&&CVPixelBufferGetPixelFormatType(out)==kCVPixelFormatType_TwoComponent16Half&&CVPixelBufferGetWidth(out)==(size_t)w/16&&CVPixelBufferGetHeight(out)==(size_t)h/16;
 if(ok){CVPixelBufferLockBaseAddress(out,kCVPixelBufferLock_ReadOnly);const uint8_t *base=CVPixelBufferGetBaseAddress(out);size_t os=CVPixelBufferGetBytesPerRow(out);
  if(!base)ok=0;else for(int y=0;y<h/16;y++)for(int x=0;x<w/16;x++){const _Float16 *v=(const _Float16*)(base+y*os);for(int k=0;k<2;k++){float f=(float)v[2*x+k];xy[(y*(w/16)+x)*2+k]=isfinite(f)&&fabsf(f)<10000?(int)lrintf(f*units):0;}}
  CVPixelBufferUnlockBaseAddress(out,kCVPixelBufferLock_ReadOnly);
 }
 if(out)CVPixelBufferRelease(out);seconds+=clock_s()-start;good+=ok;return ok;
 }else{return 0;}
}}

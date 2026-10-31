#import <Foundation/Foundation.h>
#import <VideoToolbox/VideoToolbox.h>
#import <VideoToolbox/VTMotionEstimationSession.h>
#import <VideoToolbox/VTMotionEstimationSessionProperties.h>
int main(void){@autoreleasepool{
 if(@available(macOS 26.0,*)){
  NSMutableArray *rows=[NSMutableArray new];
  int sizes[][2]={{400,176},{800,336},{1920,1080}};
  for(int i=0;i<3;i++){
   VTMotionEstimationSessionRef s=NULL;
   NSDictionary *opts=@{(__bridge NSString*)kVTMotionEstimationSessionCreationOption_MotionVectorSize:@16};
   OSStatus st=VTMotionEstimationSessionCreate(NULL,(__bridge CFDictionaryRef)opts,sizes[i][0],sizes[i][1],&s);
   NSString *desc=@""; OSStatus ast=-1;
   if(s){CFDictionaryRef attrs=NULL;ast=VTMotionEstimationSessionCopySourcePixelBufferAttributes(s,&attrs);if(attrs){desc=[(__bridge NSDictionary*)attrs description];CFRelease(attrs);}VTMotionEstimationSessionInvalidate(s);CFRelease(s);}
   [rows addObject:@{@"width":@(sizes[i][0]),@"height":@(sizes[i][1]),@"create_status":@(st),@"attributes_status":@(ast),@"attributes":desc}];
  }
  NSDictionary *report=@{@"os":NSProcessInfo.processInfo.operatingSystemVersionString,@"h264_hardware_decode_supported":@(VTIsHardwareDecodeSupported(kCMVideoCodecType_H264)),@"hevc_hardware_decode_supported":@(VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC)),@"motion_sessions":rows,@"scope":@"creation and attributes only; no frames submitted; no accelerator placement or speed measured"};
  NSData *d=[NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted error:nil];fwrite(d.bytes,1,d.length,stdout);putchar('\n');return 0;
 }else{return 2;}
}}

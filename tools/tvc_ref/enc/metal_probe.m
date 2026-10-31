/* Optional macOS research probe, never linked into tvc_enc.
 * clang -O2 -fobjc-arc -framework Foundation -framework Metal -o work/encoder_lab/metal_probe tools/tvc_ref/enc/metal_probe.m
 */
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <time.h>
#include <stdlib.h>
static double now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec * 1e-9;
}
static int cmp(const void *a, const void *b) {
    double x=*(const double *)a, y=*(const double *)b;
    return (x>y)-(x<y);
}
int main(void) { @autoreleasepool {
    id<MTLDevice> d=MTLCreateSystemDefaultDevice();
    if (!d) { fputs("Metal returned no device in this execution context\n",stderr); return 2; }
    double init=now(); NSError *err=nil;
    NSString *src=@"#include <metal_stdlib>\nusing namespace metal; kernel void tick(device uint *v [[buffer(0)]], uint i [[thread_position_in_grid]]) { if(i==0) v[0]+=1; }";
    id<MTLLibrary> lib=[d newLibraryWithSource:src options:nil error:&err];
    if (!lib) { fprintf(stderr,"Metal compilation: %s\n",err.description.UTF8String); return 3; }
    id<MTLComputePipelineState> p=[d newComputePipelineStateWithFunction:[lib newFunctionWithName:@"tick"] error:&err];
    if (!p) { fprintf(stderr,"Metal pipeline: %s\n",err.description.UTF8String); return 3; }
    id<MTLCommandQueue> q=[d newCommandQueue];
    id<MTLBuffer> b=[d newBufferWithLength:sizeof(uint32_t) options:MTLResourceStorageModeShared];
    if (!q || !b) return 3;
    double init_ms=(now()-init)*1000, wall[50], gpu[50];
    for (int i=-5; i<50; i++) {
        *(uint32_t *)b.contents=0;
        double t=now();
        id<MTLCommandBuffer> cb=[q commandBuffer];
        id<MTLComputeCommandEncoder> e=[cb computeCommandEncoder];
        [e setComputePipelineState:p]; [e setBuffer:b offset:0 atIndex:0];
        [e dispatchThreads:MTLSizeMake(1,1,1) threadsPerThreadgroup:MTLSizeMake(1,1,1)];
        [e endEncoding]; [cb commit]; [cb waitUntilCompleted];
        double dt=now()-t;
        if (cb.status!=MTLCommandBufferStatusCompleted || *(uint32_t *)b.contents!=1) {
            fprintf(stderr,"GPU execution failed: %s\n",cb.error.description.UTF8String); return 4;
        }
        if(i>=0) {wall[i]=dt*1e6; gpu[i]=(cb.GPUEndTime-cb.GPUStartTime)*1e6;}
    }
    qsort(wall,50,sizeof(double),cmp); qsort(gpu,50,sizeof(double),cmp);
    NSDictionary *report=@{@"gpu":d.name, @"unified_memory":@(d.hasUnifiedMemory),
        @"pipeline_initialization_ms":@(init_ms), @"warmups":@5, @"trials":@50,
        @"tiny_dispatch_wall_median_us":@(wall[25]), @"tiny_dispatch_wall_p95_us":@(wall[47]),
        @"tiny_dispatch_gpu_median_us":@(gpu[25]),
        @"scope":@"one uint increment; submit+wait wall time; not a TVC or throughput benchmark"};
    NSData *json=[NSJSONSerialization dataWithJSONObject:report options:NSJSONWritingPrettyPrinted error:&err];
    if(!json) return 5;
    fwrite(json.bytes,1,json.length,stdout); putchar('\n'); return 0;
}}

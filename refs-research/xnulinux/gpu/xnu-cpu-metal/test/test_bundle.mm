// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <objc/runtime.h>
#include <stdio.h>
#include <math.h>

@interface NSObject (XnuCpuMetalPlugin)
- (id)initWithAcceleratorPort:(unsigned)port;
@end

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc < 3) {
            fprintf(stderr, "Usage: %s <XnuCpuMetal.bundle> <vecadd.metallib>\n", argv[0]);
            return 2;
        }
        NSString *bundlePath = [NSString stringWithUTF8String:argv[1]];
        NSString *metallibPath = [NSString stringWithUTF8String:argv[2]];

        NSBundle *bundle = [NSBundle bundleWithPath:bundlePath];
        NSError *loadErr = nil;
        if (!bundle || ![bundle loadAndReturnError:&loadErr]) {
            fprintf(stderr, "Failed to load bundle %s: %s\n",
                    bundlePath.UTF8String, loadErr.localizedDescription.UTF8String ?: "unknown");
            return 1;
        }

        Class principal = [bundle principalClass];
        Class iogpuBase = objc_getClass("IOGPUMetalDevice");
        printf("Bundle loaded: principalClass=%s, isSubclassOf(IOGPUMetalDevice)=%s\n",
               principal ? class_getName(principal) : "(nil)",
               (principal && iogpuBase && [principal isSubclassOfClass:iogpuBase]) ? "YES" : "NO");

        if (!principal || !iogpuBase || ![principal isSubclassOfClass:iogpuBase]) {
            fprintf(stderr, "Principal class check failed!\n");
            return 1;
        }

        id<MTLDevice> dev = [[principal alloc] initWithAcceleratorPort:0];
        if (!dev) {
            fprintf(stderr, "initWithAcceleratorPort: returned nil!\n");
            return 1;
        }
        printf("Device: %s (registryID=0x%llx, lowPower=%d, headless=%d, unified=%d, conformsMTLDevice=%d)\n",
               dev.name.UTF8String, (unsigned long long)dev.registryID,
               dev.isLowPower, dev.isHeadless, dev.hasUnifiedMemory,
               [dev conformsToProtocol:@protocol(MTLDevice)]);

        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithURL:[NSURL fileURLWithPath:metallibPath] error:&err];
        if (!lib) {
            fprintf(stderr, "newLibraryWithURL failed: %s\n", err.localizedDescription.UTF8String);
            return 1;
        }
        printf("Library functions: %s\n", [[lib.functionNames componentsJoinedByString:@", "] UTF8String]);

        id<MTLFunction> fn = [lib newFunctionWithName:@"vecadd"];
        if (!fn) {
            fprintf(stderr, "newFunctionWithName(vecadd) returned nil\n");
            return 1;
        }

        id<MTLComputePipelineState> pso = [dev newComputePipelineStateWithFunction:fn error:&err];
        if (!pso) {
            fprintf(stderr, "newComputePipelineStateWithFunction failed: %s\n", err.localizedDescription.UTF8String);
            return 1;
        }

        const uint32_t N = 1024;
        float a[1024], b[1024];
        for (uint32_t i = 0; i < N; i++) {
            a[i] = (float)i * 3.0f;
            b[i] = (float)(N - i) * 0.25f;
        }
        id<MTLBuffer> bufA = [dev newBufferWithBytes:a length:sizeof(a) options:MTLResourceStorageModeShared];
        id<MTLBuffer> bufB = [dev newBufferWithBytes:b length:sizeof(b) options:MTLResourceStorageModeShared];
        id<MTLBuffer> bufC = [dev newBufferWithLength:sizeof(a) options:MTLResourceStorageModeShared];

        id<MTLCommandQueue> q = [dev newCommandQueue];
        id<MTLCommandBuffer> cb = [q commandBuffer];
        __block BOOL completedHandlerRan = NO;
        [cb addCompletedHandler:^(id<MTLCommandBuffer> _Nonnull c) {
            (void)c;
            completedHandlerRan = YES;
        }];

        id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
        [enc setComputePipelineState:pso];
        [enc setBuffer:bufA offset:0 atIndex:0];
        [enc setBuffer:bufB offset:0 atIndex:1];
        [enc setBuffer:bufC offset:0 atIndex:2];
        [enc dispatchThreads:MTLSizeMake(N, 1, 1) threadsPerThreadgroup:MTLSizeMake(64, 1, 1)];
        [enc endEncoding];

        [cb commit];
        [cb waitUntilCompleted];

        const float *c = (const float *)bufC.contents;
        uint32_t bad = 0;
        for (uint32_t i = 0; i < N; i++) {
            float exp = a[i] + b[i];
            if (fabsf(c[i] - exp) > 1e-5f) bad++;
        }
        printf("Bundle ObjC MTLDevice vecadd N=%u: status=%lu, completedHandler=%d, bad=%u (c[0]=%.2f, c[10]=%.2f, c[1023]=%.2f)\n",
               N, (unsigned long)cb.status, completedHandlerRan, bad, c[0], c[10], c[1023]);
        return (bad == 0 && completedHandlerRan) ? 0 : 1;
    }
}

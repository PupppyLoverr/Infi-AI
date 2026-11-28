// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOSurface/IOSurface.h>
#import <QuartzCore/QuartzCore.h>
#import <CoreGraphics/CoreGraphics.h>
#include <atomic>
#include <execinfo.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static void crashHandler(int sig) {
    void *bt[32];
    int n = backtrace(bt, 32);
    fprintf(stderr, "\n=== CRASH sig=%d ===\n", sig);
    backtrace_symbols_fd(bt, n, 2);
    _exit(128 + sig);
}

static id<MTLDevice> loadBundleDevice(const char *bundlePath) {
    NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
    if (!b || ![b load]) return nil;
    return (id<MTLDevice>)[(XcmBundleDeviceStub *)[b.principalClass alloc] initWithAcceleratorPort:0u];
}

int main(int argc, char **argv) {
    signal(SIGSEGV, crashHandler);
    signal(SIGBUS, crashHandler);
    signal(SIGABRT, crashHandler);
    setvbuf(stdout, nullptr, _IONBF, 0);

    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : "dist/XnuCpuMetal.bundle";
        int numThreads = (argc > 2) ? atoi(argv[2]) : 8;
        int itersPerThread = (argc > 3) ? atoi(argv[3]) : 80;

        id<MTLDevice> dev = loadBundleDevice(bundlePath);
        if (!dev) { fprintf(stderr, "FAIL: device nil\n"); return 1; }
        printf("Stress test: %s, threads=%d, iters=%d, LP_NUM_THREADS=%s\n",
               dev.name.UTF8String, numThreads, itersPerThread, getenv("LP_NUM_THREADS") ?: "(default)");

        NSString *msl =
            @"#include <metal_stdlib>\n"
             "using namespace metal;\n"
             "struct VOut { float4 pos [[position]]; float4 col; };\n"
             "vertex VOut vs(uint vid [[vertex_id]], constant float4 *tint [[buffer(0)]]) {\n"
             "  float2 p[3] = { float2(-0.8,-0.8), float2(0.8,-0.8), float2(0.0,0.8) };\n"
             "  VOut o; o.pos = float4(p[vid % 3], 0, 1); o.col = tint[0]; return o;\n"
             "}\n"
             "fragment half4 fs(VOut in [[stage_in]]) { return half4(in.col); }\n";
        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithSource:msl options:nil error:&err];
        if (!lib) { fprintf(stderr, "FAIL: lib: %s\n", err.localizedDescription.UTF8String); return 2; }
        MTLRenderPipelineDescriptor *rpd = [[MTLRenderPipelineDescriptor alloc] init];
        rpd.vertexFunction = [lib newFunctionWithName:@"vs"];
        rpd.fragmentFunction = [lib newFunctionWithName:@"fs"];
        rpd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        id<MTLRenderPipelineState> pso = [dev newRenderPipelineStateWithDescriptor:rpd error:&err];
        if (!pso) { fprintf(stderr, "FAIL: pso: %s\n", err.localizedDescription.UTF8String); return 3; }

        // Shared queue + per-thread queues
        id<MTLCommandQueue> sharedQ = [dev newCommandQueue];
        dispatch_group_t grp = dispatch_group_create();
        dispatch_queue_t concQ = dispatch_queue_create("stress.mt", DISPATCH_QUEUE_CONCURRENT);
        static std::atomic<int> completedCount{0};
        static std::atomic<int> handlerCount{0};
        static std::atomic<int> failures{0};

        // Phase 1: Heavy concurrent commit + completedHandler + waitUntilCompleted + immediate release
        for (int t = 0; t < numThreads; t++) {
            dispatch_group_async(grp, concQ, ^{
                id<MTLCommandQueue> q = (t % 2 == 0) ? sharedQ : [dev newCommandQueue];
                for (int i = 0; i < itersPerThread; i++) {
                    @autoreleasepool {
                        int sz = 64 + ((t * 17 + i * 13) % 192);
                        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                                      width:sz height:sz mipmapped:NO];
                        td.storageMode = MTLStorageModeShared;
                        td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
                        id<MTLTexture> tex = nil;
                        IOSurfaceRef surf = nullptr;
                        if (i % 3 == 0) {
                            NSDictionary *props = @{
                                (id)kIOSurfaceWidth: @(sz),
                                (id)kIOSurfaceHeight: @(sz),
                                (id)kIOSurfaceBytesPerElement: @4,
                                (id)kIOSurfacePixelFormat: @(0x42475241u),
                            };
                            surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
                            tex = [dev newTextureWithDescriptor:td iosurface:surf plane:0];
                        } else {
                            tex = [dev newTextureWithDescriptor:td];
                        }

                        id<MTLCommandBuffer> cb = [q commandBuffer];
                        [cb addScheduledHandler:^(id<MTLCommandBuffer> c) { (void)c; }];
                        [cb addCompletedHandler:^(id<MTLCommandBuffer> c) {
                            if (c.status != MTLCommandBufferStatusCompleted) failures.fetch_add(1);
                            handlerCount.fetch_add(1);
                        }];

                        MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                        rp.colorAttachments[0].texture = tex;
                        rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                        rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                        rp.colorAttachments[0].clearColor = MTLClearColorMake(0.1, 0.2, 0.3, 1.0);
                        id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                        [enc setRenderPipelineState:pso];
                        float tint[4] = {1.0f, 0.5f, 0.25f, 1.0f};
                        [enc setVertexBytes:tint length:sizeof(tint) atIndex:0];
                        [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:30];
                        [enc endEncoding];

                        [cb commit];
                        if (i % 2 == 0) {
                            [cb waitUntilCompleted];
                            if (cb.status != MTLCommandBufferStatusCompleted) failures.fetch_add(1);
                        }
                        // Immediately let cb, tex, surf drop out of autoreleasepool!
                        if (surf) CFRelease(surf);
                        completedCount.fetch_add(1);
                    }
                }
            });
        }
        dispatch_group_wait(grp, DISPATCH_TIME_FOREVER);
        // Drain any non-waited CBs
        usleep(300000);
        printf("Phase 1 done: submitted=%d handlers=%d failures=%d\n",
               completedCount.load(), handlerCount.load(), failures.load());

        // Phase 2: Concurrent CARenderer across 4 threads (mimics iconservicesagent rendering icons concurrently)
        static std::atomic<int> caDone{0};
        for (int t = 0; t < 4; t++) {
            dispatch_group_async(grp, concQ, ^{
                for (int i = 0; i < 15; i++) {
                    @autoreleasepool {
                        int W = 128, H = 128;
                        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                                      width:W height:H mipmapped:NO];
                        td.storageMode = MTLStorageModeShared;
                        td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
                        id<MTLTexture> tex = [dev newTextureWithDescriptor:td];

                        CALayer *root = [CALayer layer];
                        root.bounds = CGRectMake(0, 0, W, H);
                        root.position = CGPointMake(W / 2.0, H / 2.0);
                        CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
                        CGFloat bgc[4] = {0.9, 0.9, 0.95, 1.0};
                        CGColorRef bg = CGColorCreate(cs, bgc);
                        root.backgroundColor = bg;
                        CGColorRelease(bg);
                        CALayer *child = [CALayer layer];
                        child.frame = CGRectMake(24, 24, W - 48, H - 48);
                        CGFloat cc[4] = {(t % 2) ? 0.9 : 0.2, 0.4, 0.8, 1.0};
                        CGColorRef fg = CGColorCreate(cs, cc);
                        child.backgroundColor = fg;
                        CGColorRelease(fg);
                        child.cornerRadius = 20.0;
                        child.shadowOpacity = 0.6f;
                        child.shadowRadius = 6.0;
                        [root addSublayer:child];
                        CGColorSpaceRelease(cs);

                        CARenderer *r = [CARenderer rendererWithMTLTexture:tex options:nil];
                        r.layer = root;
                        r.bounds = CGRectMake(0, 0, W, H);
                        [CATransaction flush];
                        [r beginFrameAtTime:CACurrentMediaTime() timeStamp:NULL];
                        [r addUpdateRect:r.bounds];
                        [r render];
                        [r endFrame];
                        caDone.fetch_add(1);
                    }
                }
            });
        }
        dispatch_group_wait(grp, DISPATCH_TIME_FOREVER);
        usleep(300000);
        printf("Phase 2 done: caDone=%d failures=%d\n", caDone.load(), failures.load());

        // Phase 3: newBufferWithBytesNoCopy + munmap deallocator + immediate release after commit
        static std::atomic<int> noCopyDone{0};
        static std::atomic<int> deallocCount{0};
        for (int t = 0; t < 4; t++) {
            dispatch_group_async(grp, concQ, ^{
                id<MTLCommandQueue> q = [dev newCommandQueue];
                for (int i = 0; i < 40; i++) {
                    @autoreleasepool {
                        size_t pg = 16384;
                        void *mem = mmap(nullptr, pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
                        if (mem == MAP_FAILED) { failures.fetch_add(1); continue; }
                        float *fp = (float *)mem;
                        fp[0] = 0.25f; fp[1] = 0.75f; fp[2] = 0.5f; fp[3] = 1.0f;
                        id<MTLBuffer> buf = [dev newBufferWithBytesNoCopy:mem length:pg options:MTLResourceStorageModeShared deallocator:^(void *ptr, NSUInteger len) {
                            munmap(ptr, len);
                            deallocCount.fetch_add(1);
                        }];
                        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                                      width:64 height:64 mipmapped:NO];
                        td.storageMode = MTLStorageModeShared;
                        td.usage = MTLTextureUsageRenderTarget;
                        id<MTLTexture> tex = [dev newTextureWithDescriptor:td];
                        id<MTLCommandBuffer> cb = [q commandBuffer];
                        MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                        rp.colorAttachments[0].texture = tex;
                        rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                        rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                        id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                        [enc setRenderPipelineState:pso];
                        [enc setVertexBuffer:buf offset:0 atIndex:0];
                        [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:30];
                        [enc endEncoding];
                        [cb commit];
                        if (i % 2 == 0) [cb waitUntilCompleted];
                        // Drop buf, tex, cb immediately!
                        noCopyDone.fetch_add(1);
                    }
                }
            });
        }
        dispatch_group_wait(grp, DISPATCH_TIME_FOREVER);
        usleep(400000);
        printf("Phase 3 done: noCopyDone=%d deallocCount=%d failures=%d\n",
               noCopyDone.load(), deallocCount.load(), failures.load());
        if (deallocCount.load() != noCopyDone.load()) {
            fprintf(stderr, "FAIL: deallocCount (%d) != noCopyDone (%d)\n", deallocCount.load(), noCopyDone.load());
            failures.fetch_add(1);
        }

        // Phase 4: Handler ordering invariant under concurrent waitUntilCompleted
        for (int i = 0; i < 40; i++) {
            @autoreleasepool {
                MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                              width:32 height:32 mipmapped:NO];
                td.storageMode = MTLStorageModeShared;
                td.usage = MTLTextureUsageRenderTarget;
                id<MTLTexture> tex = [dev newTextureWithDescriptor:td];
                id<MTLCommandBuffer> cb = [sharedQ commandBuffer];
                std::atomic<int> hDone{0};
                std::atomic<int> hCalls{0};
                std::atomic<int> *pDone = &hDone;
                std::atomic<int> *pCalls = &hCalls;
                [cb addCompletedHandler:^(id<MTLCommandBuffer> c) {
                    (void)c;
                    usleep(500);
                    pCalls->fetch_add(1);
                    pDone->store(1);
                }];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = tex;
                rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc setRenderPipelineState:pso];
                float tint[4] = {1, 0, 0, 1};
                [enc setVertexBytes:tint length:sizeof(tint) atIndex:0];
                [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
                [enc endEncoding];
                [cb commit];
                dispatch_group_t g2 = dispatch_group_create();
                dispatch_group_async(g2, concQ, ^{
                    [cb waitUntilCompleted];
                    if (pDone->load() != 1) failures.fetch_add(1);
                });
                [cb waitUntilCompleted];
                if (pDone->load() != 1) failures.fetch_add(1);
                dispatch_group_wait(g2, DISPATCH_TIME_FOREVER);
                if (pCalls->load() != 1) failures.fetch_add(1);
            }
        }
        printf("Phase 4 done: failures=%d\n", failures.load());
        return failures.load() ? 1 : 0;
    }
}

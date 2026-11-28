// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOSurface/IOSurface.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static double nowMs() {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec * 1e-6;
}

static void benchSize(id<MTLDevice> dev, id<MTLCommandQueue> q, int W, int H, int iters) {
    @autoreleasepool {
        NSDictionary *props = @{
            (id)kIOSurfaceWidth: @(W),
            (id)kIOSurfaceHeight: @(H),
            (id)kIOSurfaceBytesPerElement: @4,
            (id)kIOSurfacePixelFormat: @(0x42475241u),
        };
        IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
        size_t bytes = (size_t)W * (size_t)H * 4;

        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                      width:W height:H mipmapped:NO];
        td.storageMode = MTLStorageModeShared;
        td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        id<MTLTexture> tex = [dev newTextureWithDescriptor:td iosurface:surf plane:0];

        // Warmup
        for (int i = 0; i < 3; i++) {
            id<MTLCommandBuffer> cb = [q commandBuffer];
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = tex;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(0.2, 0.4, 0.6, 1.0);
            id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
            [enc endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
        }

        // Measure Clear + Store (includes syncToIOSurface getBytes copy)
        double t0 = nowMs();
        for (int i = 0; i < iters; i++) {
            @autoreleasepool {
                id<MTLCommandBuffer> cb = [q commandBuffer];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = tex;
                rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                rp.colorAttachments[0].clearColor = MTLClearColorMake(0.1 * (i % 5), 0.4, 0.6, 1.0);
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
            }
        }
        double storeMs = (nowMs() - t0) / iters;

        // Measure Load (CPU mutated IOSurface -> syncFromIOSurfaceIfNeeded replaceRegion) + Store (syncToIOSurface getBytes)
        double t1 = nowMs();
        for (int i = 0; i < iters; i++) {
            @autoreleasepool {
                IOSurfaceLock(surf, 0, nullptr);
                uint32_t *p = (uint32_t *)IOSurfaceGetBaseAddress(surf);
                p[0] = (uint32_t)i;
                IOSurfaceUnlock(surf, 0, nullptr); // increments IOSurface seed -> triggers upload on Load

                id<MTLCommandBuffer> cb = [q commandBuffer];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = tex;
                rp.colorAttachments[0].loadAction = MTLLoadActionLoad;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
            }
        }
        double roundtripMs = (nowMs() - t1) / iters;
        double uploadEstMs = roundtripMs - storeMs;
        double mb = bytes / (1024.0 * 1024.0);
        printf("  %4dx%-4d (%.2f MB): syncToIOSurface(Store)=%.2f ms (%.0f MB/s), upload+download=%.2f ms (est upload=%.2f ms)\n",
               W, H, mb, storeMs, storeMs > 0 ? mb / (storeMs / 1000.0) : 0.0, roundtripMs, uploadEstMs);
        CFRelease(surf);
    }
}

int main(int argc, char **argv) {
    setvbuf(stdout, nullptr, _IONBF, 0);
    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : "dist/XnuCpuMetal.bundle";
        NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
        if (!b || ![b load]) { fprintf(stderr, "FAIL: load bundle\n"); return 1; }
        id<MTLDevice> dev = (id<MTLDevice>)[(XcmBundleDeviceStub *)[b.principalClass alloc] initWithAcceleratorPort:0u];
        if (!dev) { fprintf(stderr, "FAIL: device nil\n"); return 2; }
        printf("IOSurface sync bench on: %s\n", dev.name.UTF8String);
        id<MTLCommandQueue> q = [dev newCommandQueue];
        benchSize(dev, q, 512, 512, 30);
        benchSize(dev, q, 1920, 1080, 20);
        benchSize(dev, q, 2560, 1440, 15);
        return 0;
    }
}

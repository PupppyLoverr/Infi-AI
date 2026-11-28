// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOSurface/IOSurface.h>
#include <dlfcn.h>
#include <objc/message.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <execinfo.h>
#include <signal.h>
static void crash_h(int sig){ void *bt[32]; int n=backtrace(bt,32); fprintf(stderr,"CRASH sig=%d\n",sig); backtrace_symbols_fd(bt,n,2); _exit(128+sig); }


@protocol XcmLinearBufferSPI <MTLBuffer>
- (id<MTLTexture>)newLinearTextureWithDescriptor:(MTLTextureDescriptor *)desc
                                          offset:(NSUInteger)offset
                                     bytesPerRow:(NSUInteger)bytesPerRow
                                   bytesPerImage:(NSUInteger)bytesPerImage;
@end

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static id<MTLDevice> loadBundleDevice(const char *bundlePath) {
    NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
    if (!b) { fprintf(stderr, "FAIL: bundle not found at %s\n", bundlePath); return nil; }
    NSError *err = nil;
    if (![b loadAndReturnError:&err]) {
        fprintf(stderr, "FAIL: bundle load: %s\n", err.localizedDescription.UTF8String);
        return nil;
    }
    Class cls = b.principalClass;
    id dev = [(XcmBundleDeviceStub *)[cls alloc] initWithAcceleratorPort:0u];
    return (id<MTLDevice>)dev;
}

static void *allocPages(size_t len) {
    void *p = mmap(nullptr, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    return (p == MAP_FAILED) ? nullptr : p;
}

int main(int argc, char **argv) {
    signal(SIGSEGV, crash_h); signal(SIGBUS, crash_h); setvbuf(stdout, nullptr, _IONBF, 0);
    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : "dist/XnuCpuMetal.bundle";
        id<MTLDevice> dev = loadBundleDevice(bundlePath);
        if (!dev) { fprintf(stderr, "FAIL: device open returned nil\n"); return 1; }
        printf("Loaded device: %s\n", dev.name.UTF8String);

        // ---- Test 1: CAEnableIOSurfaceCompression in-memory only (no plist on disk) ----
        {
            Boolean valid = false;
            Boolean val = CFPreferencesGetAppBooleanValue(
                CFSTR("CAEnableIOSurfaceCompression"), kCFPreferencesCurrentApplication, &valid);
            if (!valid || val) {
                fprintf(stderr, "FAIL: CAEnableIOSurfaceCompression valid=%d val=%d (expected 1, 0)\n",
                        (int)valid, (int)val);
                return 2;
            }
            NSString *plistPath = [NSHomeDirectory() stringByAppendingPathComponent:
                                   @"Library/Preferences/test_round10.plist"];
            struct stat st;
            if (stat(plistPath.fileSystemRepresentation, &st) == 0) {
                fprintf(stderr, "FAIL: unexpected plist created on disk at %s\n", plistPath.UTF8String);
                return 3;
            }
            printf("PASS [1/4]: CAEnableIOSurfaceCompression=NO in-memory only (no plist on disk)\n");
        }

        // ---- Test 2: WindowServer 4 buffer-backed linear texture shapes + live CPU mutation ----
        id<MTLCommandQueue> q = [dev newCommandQueue];
        if (!q) { fprintf(stderr, "FAIL: newCommandQueue nil\n"); return 4; }

        {
            struct Shape {
                const char *name;
                size_t bufLen;
                NSUInteger offset;
                NSUInteger w, h, bpr, bpi;
                MTLPixelFormat fmt;
            } shapes[] = {
                {"84x103_rg8_o0",      0x5000, 0x0000,  84, 103,  192, 19752, MTLPixelFormatRG8Unorm},
                {"234x18_bgra_o100",   0x5000, 0x0100, 234,  18,  960, 17256, MTLPixelFormatBGRA8Unorm},
                {"256x1_rgba16f_o0",   0x1000, 0x0000, 256,   1, 2048,  2048, MTLPixelFormatRGBA16Float},
                {"1x1_bgra_o100",      0x1000, 0x0100,   1,   1,   64,     4, MTLPixelFormatBGRA8Unorm},
            };
            for (const auto &s : shapes) {
                void *mem = allocPages(s.bufLen);
                if (!mem) { fprintf(stderr, "FAIL: mmap %s\n", s.name); return 5; }
                memset(mem, 0x5A, s.bufLen);
                __block BOOL deallocCalled = NO;
                @autoreleasepool {
                    id<MTLBuffer> buf = [dev newBufferWithBytesNoCopy:mem
                                                               length:s.bufLen
                                                              options:MTLResourceStorageModeShared
                                                          deallocator:^(void *ptr, NSUInteger len) {
                        deallocCalled = YES;
                        munmap(ptr, len);
                    }];
                    if (!buf) { fprintf(stderr, "FAIL: no-copy buffer nil for %s\n", s.name); return 6; }
                    MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:s.fmt
                                                                                                  width:s.w
                                                                                                 height:s.h
                                                                                              mipmapped:NO];
                    td.storageMode = MTLStorageModeShared;
                    td.usage = MTLTextureUsageShaderRead;
                    id<MTLTexture> tex = [(id<XcmLinearBufferSPI>)buf
                        newLinearTextureWithDescriptor:td offset:s.offset bytesPerRow:s.bpr bytesPerImage:s.bpi];
                    if (!tex) { fprintf(stderr, "FAIL: newLinearTexture returned nil for %s\n", s.name); return 7; }
                    if (tex.buffer != buf || tex.bufferOffset != s.offset || tex.bufferBytesPerRow != s.bpr) {
                        fprintf(stderr, "FAIL: metadata mismatch for %s (buf=%p/%p off=%lu/%lu bpr=%lu/%lu)\n",
                                s.name, (__bridge void *)tex.buffer, (__bridge void *)buf,
                                (unsigned long)tex.bufferOffset, (unsigned long)s.offset,
                                (unsigned long)tex.bufferBytesPerRow, (unsigned long)s.bpr);
                        return 8;
                    }
                    // Also test standard MTLBuffer newTextureWithDescriptor:offset:bytesPerRow:
                    id<MTLTexture> tex2 = [buf newTextureWithDescriptor:td offset:s.offset bytesPerRow:s.bpr];
                    if (!tex2) { fprintf(stderr, "FAIL: newTextureWithDescriptor returned nil for %s\n", s.name); return 9; }
                }
                if (!deallocCalled) {
                    fprintf(stderr, "FAIL: no-copy buffer deallocator not called for %s\n", s.name);
                    return 10;
                }
            }

            // Live CPU mutation test with shader sampling -> IOSurface render target
            const NSUInteger W = 234, H = 18, BPR = 960, OFF = 0x100, BLEN = 0x5000;
            void *mem = allocPages(BLEN);
            id<MTLBuffer> buf = [dev newBufferWithBytesNoCopy:mem length:BLEN options:MTLResourceStorageModeShared
                                                  deallocator:^(void *ptr, NSUInteger len) { munmap(ptr, len); }];
            MTLTextureDescriptor *srcDesc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                               width:W height:H mipmapped:NO];
            srcDesc.storageMode = MTLStorageModeShared;
            srcDesc.usage = MTLTextureUsageShaderRead;
            id<MTLTexture> srcTex = [(id<XcmLinearBufferSPI>)buf
                newLinearTextureWithDescriptor:srcDesc offset:OFF bytesPerRow:BPR bytesPerImage:BPR * H];

            MTLTextureDescriptor *dstDesc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                               width:16 height:16 mipmapped:NO];
            dstDesc.storageMode = MTLStorageModeShared;
            dstDesc.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
            id<MTLTexture> dstTex = [dev newTextureWithDescriptor:dstDesc];

            NSString *msl =
                @"#include <metal_stdlib>\n"
                 "using namespace metal;\n"
                 "struct VOut { float4 pos [[position]]; float2 uv; };\n"
                 "vertex VOut vs(uint vid [[vertex_id]]) {\n"
                 "  float2 p[3] = { float2(-1,-1), float2(3,-1), float2(-1,3) };\n"
                 "  VOut o; o.pos = float4(p[vid], 0, 1); o.uv = float2(0.5, 0.5); return o;\n"
                 "}\n"
                 "fragment half4 fs(VOut in [[stage_in]], texture2d<half> t [[texture(0)]]) {\n"
                 "  constexpr sampler s(filter::nearest);\n"
                 "  return t.sample(s, in.uv);\n"
                 "}\n";
            NSError *err = nil;
            id<MTLLibrary> lib = [dev newLibraryWithSource:msl options:nil error:&err];
            if (!lib) { fprintf(stderr, "FAIL: shader compile: %s\n", err.localizedDescription.UTF8String); return 11; }
            MTLRenderPipelineDescriptor *rpd = [[MTLRenderPipelineDescriptor alloc] init];
            rpd.vertexFunction = [lib newFunctionWithName:@"vs"];
            rpd.fragmentFunction = [lib newFunctionWithName:@"fs"];
            rpd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
            id<MTLRenderPipelineState> pso = [dev newRenderPipelineStateWithDescriptor:rpd error:&err];
            if (!pso) { fprintf(stderr, "FAIL: PSO: %s\n", err.localizedDescription.UTF8String); return 12; }

            auto fillAndRender = [&](uint32_t bgraPixel) -> uint32_t {
                uint8_t *base = (uint8_t *)mem + OFF;
                for (NSUInteger y = 0; y < H; y++) {
                    uint32_t *row = (uint32_t *)(base + y * BPR);
                    for (NSUInteger x = 0; x < W; x++) row[x] = bgraPixel;
                }
                id<MTLCommandBuffer> cb = [q commandBuffer];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = dstTex;
                rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                rp.colorAttachments[0].clearColor = MTLClearColorMake(0, 0, 0, 0);
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc setRenderPipelineState:pso];
                [enc setFragmentTexture:srcTex atIndex:0];
                [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
                if (cb.status != MTLCommandBufferStatusCompleted) {
                    fprintf(stderr, "FAIL: render CB status=%lu err=%s\n",
                            (unsigned long)cb.status, cb.error.localizedDescription.UTF8String);
                    return 0;
                }
                uint32_t outPx = 0;
                [dstTex getBytes:&outPx bytesPerRow:16 * 4 fromRegion:MTLRegionMake2D(8, 8, 1, 1) mipmapLevel:0];
                return outPx;
            };

            uint32_t p1 = fillAndRender(0xFF112233u);
            uint32_t p2 = fillAndRender(0xFF44CC88u);
            if (p1 != 0xFF112233u || p2 != 0xFF44CC88u) {
                fprintf(stderr, "FAIL: live buffer mutation mismatch: frame1=0x%08x (exp 0xFF112233) frame2=0x%08x (exp 0xFF44CC88)\n", p1, p2);
                return 13;
            }
            printf("PASS [2/4]: All 4 WindowServer linear texture shapes + live CPU mutation (0x%08x -> 0x%08x) + deallocator\n", p1, p2);
        }

        // ---- Test 3: MTLSharedEvent + MTLSharedEventListener + Handle ----
        {
            id<MTLSharedEvent> ev = [dev newSharedEvent];
            if (!ev) { fprintf(stderr, "FAIL: newSharedEvent returned nil\n"); return 14; }
            if (![ev conformsToProtocol:@protocol(MTLSharedEvent)]) {
                fprintf(stderr, "FAIL: event does not conform to MTLSharedEvent\n"); return 15;
            }
            if (ev.signaledValue != 0) {
                fprintf(stderr, "FAIL: initial signaledValue=%llu\n", ev.signaledValue); return 16;
            }
            dispatch_queue_t notifyQ = dispatch_queue_create("test.shared.event", DISPATCH_QUEUE_SERIAL);
            MTLSharedEventListener *listener = [[MTLSharedEventListener alloc] initWithDispatchQueue:notifyQ];
            dispatch_semaphore_t sem = dispatch_semaphore_create(0);
            __block uint64_t seenVal = 0;
            [ev notifyListener:listener atValue:1 block:^(id<MTLSharedEvent> e, uint64_t val) {
                (void)e;
                seenVal = val;
                dispatch_semaphore_signal(sem);
            }];

            id<MTLCommandBuffer> cb1 = [q commandBuffer];
            [cb1 encodeSignalEvent:ev value:1];
            [cb1 commit];
            [cb1 waitUntilCompleted];
            if (cb1.status != MTLCommandBufferStatusCompleted) {
                fprintf(stderr, "FAIL: cb1 status=%lu err=%s\n",
                        (unsigned long)cb1.status, cb1.error.localizedDescription.UTF8String);
                return 17;
            }
            if (dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)) != 0 || seenVal < 1) {
                fprintf(stderr, "FAIL: MTLSharedEventListener did not fire (seenVal=%llu)\n", seenVal);
                return 18;
            }
            if (![ev waitUntilSignaledValue:1 timeoutMS:1000]) {
                fprintf(stderr, "FAIL: waitUntilSignaledValue(1) returned NO\n"); return 19;
            }

            // Wait on value 2 -> signal value 3, unblocked from CPU via setSignaledValue:2
            id<MTLCommandBuffer> cb2 = [q commandBuffer];
            [cb2 encodeWaitForEvent:ev value:2];
            [cb2 encodeSignalEvent:ev value:3];
            [cb2 commit];
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 50 * NSEC_PER_MSEC),
                           dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
                ev.signaledValue = 2;
            });
            [cb2 waitUntilCompleted];
            if (cb2.status != MTLCommandBufferStatusCompleted || ev.signaledValue != 3) {
                fprintf(stderr, "FAIL: cb2 status=%lu val=%llu\n", (unsigned long)cb2.status, ev.signaledValue);
                return 20;
            }

            // Handle round-trip
            MTLSharedEventHandle *h = [ev newSharedEventHandle];
            id<MTLSharedEvent> evAlias = h ? [dev newSharedEventWithHandle:h] : nil;
            if (!evAlias || evAlias.signaledValue != 3) {
                fprintf(stderr, "FAIL: shared event handle alias failed (evAlias=%p val=%llu)\n",
                        (__bridge void *)evAlias, evAlias ? evAlias.signaledValue : 0);
                return 21;
            }
            printf("PASS [3/4]: MTLSharedEvent + MTLSharedEventListener + CPU wait/signal + Handle alias\n");
        }

        // ---- Test 4: MTLFence across Blit -> Compute encoders ----
        {
            id<MTLFence> fence = [dev newFence];
            id<MTLBuffer> b1 = [dev newBufferWithLength:256 options:MTLResourceStorageModeShared];
            id<MTLBuffer> b2 = [dev newBufferWithLength:256 options:MTLResourceStorageModeShared];
            memset(b1.contents, 0xAB, 256);
            memset(b2.contents, 0x00, 256);
            id<MTLCommandBuffer> cb = [q commandBuffer];
            id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
            [blit copyFromBuffer:b1 sourceOffset:0 toBuffer:b2 destinationOffset:0 size:256];
            [blit updateFence:fence];
            [blit endEncoding];
            id<MTLBlitCommandEncoder> blit2 = [cb blitCommandEncoder];
            [blit2 waitForFence:fence];
            [blit2 endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
            if (cb.status != MTLCommandBufferStatusCompleted || ((uint8_t *)b2.contents)[0] != 0xAB) {
                fprintf(stderr, "FAIL: MTLFence blit test failed (status=%lu byte0=0x%02x)\n",
                        (unsigned long)cb.status, ((uint8_t *)b2.contents)[0]);
                return 22;
            }
            printf("PASS [4/4]: MTLFence updateFence/waitForFence\n");
        }

        printf("ALL ROUND 10 TESTS PASSED!\n");
        return 0;
    }
}

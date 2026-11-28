// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <atomic>
#include <execinfo.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>

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

static double nowSec() {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec * 1e-9;
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
        const char *mode = (argc > 2) ? argv[2] : "normal";
        id<MTLDevice> dev = loadBundleDevice(bundlePath);
        if (!dev) { fprintf(stderr, "FAIL: device nil\n"); return 1; }
        printf("Round 13 test (%s): %s, LP_NUM_THREADS=%s, XNU_CPU_METAL_WATCHDOG_MS=%s\n",
               mode, dev.name.UTF8String,
               getenv("LP_NUM_THREADS") ?: "(default)",
               getenv("XNU_CPU_METAL_WATCHDOG_MS") ?: "(default=3600s)");

        // Test 1: Zero-length / NULL buffer semantics (must match Apple Metal: return nil, no deallocator call)
        __block int zeroDealloc = 0;
        id<MTLBuffer> z1 = [dev newBufferWithBytesNoCopy:nullptr length:0 options:0 deallocator:^(void *, NSUInteger) {
            zeroDealloc++;
        }];
        id<MTLBuffer> z2 = [dev newBufferWithLength:0 options:0];
        id<MTLBuffer> z3 = [dev newBufferWithBytes:nullptr length:0 options:0];
        if (z1 != nil || z2 != nil || z3 != nil || zeroDealloc != 0) {
            fprintf(stderr, "FAIL [1]: zero-length buffers z1=%p z2=%p z3=%p dealloc=%d\n",
                    (__bridge void *)z1, (__bridge void *)z2, (__bridge void *)z3, zeroDealloc);
            return 2;
        }
        printf("PASS [1]: newBufferWithBytesNoCopy(NULL,0) / newBufferWithLength(0) / newBufferWithBytes(NULL,0) all return nil without calling deallocator\n");

        // Heavy vertex+fragment shader that reads from a vertex buffer and does heavy ALU work
        // so we can reliably control single-CB duration on llvmpipe.
        NSString *msl =
            @"#include <metal_stdlib>\n"
             "using namespace metal;\n"
             "struct VIn { float4 pos; float4 col; };\n"
             "struct VOut { float4 pos [[position]]; float4 col; };\n"
             "vertex VOut vs(uint vid [[vertex_id]], device const VIn *verts [[buffer(0)]]) {\n"
             "  VOut o; o.pos = verts[vid].pos; o.col = verts[vid].col; return o;\n"
             "}\n"
             "fragment float4 fs(VOut in [[stage_in]], constant uint &iters [[buffer(0)]]) {\n"
             "  float4 acc = in.col;\n"
             "  for (uint i = 0; i < iters; i++) {\n"
             "    acc = sin(acc * 1.0001f + float4(0.1f, 0.2f, 0.3f, 0.4f));\n"
             "  }\n"
             "  return float4(abs(acc.rgb), 1.0f);\n"
             "}\n";
        NSError *err = nil;
        id<MTLLibrary> lib = [dev newLibraryWithSource:msl options:nil error:&err];
        if (!lib) { fprintf(stderr, "FAIL: lib: %s\n", err.localizedDescription.UTF8String); return 3; }
        MTLRenderPipelineDescriptor *rpd = [[MTLRenderPipelineDescriptor alloc] init];
        rpd.vertexFunction = [lib newFunctionWithName:@"vs"];
        rpd.fragmentFunction = [lib newFunctionWithName:@"fs"];
        rpd.colorAttachments[0].pixelFormat = MTLPixelFormatBGRA8Unorm;
        id<MTLRenderPipelineState> pso = [dev newRenderPipelineStateWithDescriptor:rpd error:&err];
        if (!pso) { fprintf(stderr, "FAIL: pso: %s\n", err.localizedDescription.UTF8String); return 4; }

        id<MTLCommandQueue> q = [dev newCommandQueue];
        MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                      width:512 height:512 mipmapped:NO];
        td.storageMode = MTLStorageModeShared;
        td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
        id<MTLTexture> tex = [dev newTextureWithDescriptor:td];

        // Build mmap-backed NoCopy vertex buffer with 6000 vertices (2000 full-screen triangles)
        // placed right up to a page boundary so any UAF after munmap hits SIGSEGV immediately.
        struct VIn { float pos[4]; float col[4]; };
        const int numVerts = 600;
        size_t vbytes = numVerts * sizeof(VIn);
        size_t pg = 16384;
        size_t allocSz = (vbytes + pg - 1) & ~(pg - 1);

        auto makeVBuf = ^id<MTLBuffer>(std::atomic<int> *deallocCounter) {
            void *mem = mmap(nullptr, allocSz, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
            if (mem == MAP_FAILED) return nil;
            VIn *v = (VIn *)mem;
            float tri[3][2] = { {-1.0f, -1.0f}, {1.0f, -1.0f}, {0.0f, 1.0f} };
            for (int i = 0; i < numVerts; i++) {
                v[i].pos[0] = tri[i % 3][0]; v[i].pos[1] = tri[i % 3][1]; v[i].pos[2] = 0; v[i].pos[3] = 1;
                v[i].col[0] = 0.3f; v[i].col[1] = 0.6f; v[i].col[2] = 0.9f; v[i].col[3] = 1.0f;
            }
            return [dev newBufferWithBytesNoCopy:mem length:allocSz options:MTLResourceStorageModeShared
                                     deallocator:^(void *ptr, NSUInteger len) {
                                         munmap(ptr, len);
                                         if (deallocCounter) deallocCounter->fetch_add(1);
                                     }];
        };

        if (strcmp(mode, "hang_uaf") == 0) {
            // Test 2 (forced watchdog mode): XNU_CPU_METAL_WATCHDOG_MS=1000 while draw runs ~3s.
            // Must NOT SIGSEGV when watchdog fires and parks device!
            static std::atomic<int> dealloced{0};
            double t0 = nowSec();
            @autoreleasepool {
                id<MTLBuffer> vb = makeVBuf(&dealloced);
                id<MTLCommandBuffer> cb = [q commandBuffer];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = tex;
                rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc setRenderPipelineState:pso];
                [enc setVertexBuffer:vb offset:0 atIndex:0];
                uint32_t iters = 80;
                [enc setFragmentBytes:&iters length:sizeof(iters) atIndex:0];
                for (int d = 0; d < 20; d++) {
                    [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:numVerts];
                }
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
                printf("hang_uaf cb done in %.2fs: status=%lu err=%s dealloced=%d\n",
                       nowSec() - t0, (unsigned long)cb.status,
                       cb.error ? cb.error.localizedDescription.UTF8String : "none",
                       dealloced.load());
            }
            // Wait 3s while lavapipe finishes executing the parked draw — must NOT SIGSEGV!
            sleep(3);
            printf("PASS [hang_uaf]: survived parked-device watchdog without SIGSEGV (dealloced=%d)\n", dealloced.load());
            return 0;
        }

        // Test 2 (normal CPU mode): continuous heavy draws + slow CB (>21s? or configurable via argv[3])
        // Default: run a single heavy batch + follow-up batches to verify no GpuHang / AccessRevoked.
        uint32_t iters = (argc > 3) ? (uint32_t)atoi(argv[3]) : 40;
        int drawsPerCb = (argc > 4) ? atoi(argv[4]) : 8;
        int numCbs = (argc > 5) ? atoi(argv[5]) : 4;
        static std::atomic<int> normDealloc{0};
        double t0 = nowSec();
        for (int c = 0; c < numCbs; c++) {
            @autoreleasepool {
                id<MTLBuffer> vb = makeVBuf(&normDealloc);
                id<MTLCommandBuffer> cb = [q commandBuffer];
                MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
                rp.colorAttachments[0].texture = tex;
                rp.colorAttachments[0].loadAction = MTLLoadActionClear;
                rp.colorAttachments[0].storeAction = MTLStoreActionStore;
                id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
                [enc setRenderPipelineState:pso];
                [enc setVertexBuffer:vb offset:0 atIndex:0];
                [enc setFragmentBytes:&iters length:sizeof(iters) atIndex:0];
                for (int d = 0; d < drawsPerCb; d++) {
                    [enc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:numVerts];
                }
                [enc endEncoding];
                [cb commit];
                [cb waitUntilCompleted];
                if (cb.status != MTLCommandBufferStatusCompleted) {
                    fprintf(stderr, "FAIL [2]: cb %d status=%lu err=%s\n", c, (unsigned long)cb.status,
                            cb.error ? cb.error.localizedDescription.UTF8String : "none");
                    return 5;
                }
            }
        }
        usleep(200000);
        printf("PASS [2]: %d heavy CBs (%d draws/CB, iters=%u) completed in %.2fs with 0 GpuHang/AccessRevoked, dealloced=%d/%d\n",
               numCbs, drawsPerCb, iters, nowSec() - t0, normDealloc.load(), numCbs);
        if (normDealloc.load() != numCbs) {
            fprintf(stderr, "FAIL [2]: dealloced=%d != %d\n", normDealloc.load(), numCbs);
            return 6;
        }
        return 0;
    }
}

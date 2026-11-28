// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <CoreImage/CoreImage.h>
#import <objc/runtime.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <stdlib.h>

@interface NSObject (XnuCpuMetalPlugin)
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static int run_render_pass_test(id<MTLDevice> dev, NSString *triMetallibPath) {
    printf("\n=== [Test A] Render Pass (clear blue + orange triangle -> 64x64 RGBA8) ===\n");
    NSError *err = nil;
    id<MTLLibrary> lib = [dev newLibraryWithURL:[NSURL fileURLWithPath:triMetallibPath] error:&err];
    if (!lib) {
        fprintf(stderr, "Render: newLibraryWithURL failed: %s\n", err.localizedDescription.UTF8String);
        return 1;
    }
    id<MTLFunction> vfn = [lib newFunctionWithName:@"tri_vertex"];
    id<MTLFunction> ffn = [lib newFunctionWithName:@"tri_fragment"];
    if (!vfn || !ffn) {
        fprintf(stderr, "Render: missing vertex/fragment function\n");
        return 1;
    }

    MTLRenderPipelineDescriptor *rpd = [[MTLRenderPipelineDescriptor alloc] init];
    rpd.vertexFunction = vfn;
    rpd.fragmentFunction = ffn;
    rpd.colorAttachments[0].pixelFormat = MTLPixelFormatRGBA8Unorm;
    id<MTLRenderPipelineState> pso = [dev newRenderPipelineStateWithDescriptor:rpd error:&err];
    if (!pso) {
        fprintf(stderr, "Render: newRenderPipelineState failed: %s\n", err.localizedDescription.UTF8String);
        return 1;
    }

    const NSUInteger W = 64, H = 64;
    MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA8Unorm
                                                                                  width:W height:H mipmapped:NO];
    td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
    td.storageMode = MTLStorageModePrivate;
    id<MTLTexture> target = [dev newTextureWithDescriptor:td];
    id<MTLBuffer> readback = [dev newBufferWithLength:W * H * 4 options:MTLResourceStorageModeShared];
    if (!target || !readback) {
        fprintf(stderr, "Render: texture or readback buffer alloc failed\n");
        return 1;
    }

    MTLRenderPassDescriptor *pass = [MTLRenderPassDescriptor renderPassDescriptor];
    pass.colorAttachments[0].texture = target;
    pass.colorAttachments[0].loadAction = MTLLoadActionClear;
    pass.colorAttachments[0].storeAction = MTLStoreActionStore;
    pass.colorAttachments[0].clearColor = MTLClearColorMake(0.0, 0.0, 1.0, 1.0);

    id<MTLCommandQueue> q = [dev newCommandQueue];
    id<MTLCommandBuffer> cb = [q commandBuffer];
    id<MTLRenderCommandEncoder> renc = [cb renderCommandEncoderWithDescriptor:pass];
    [renc setRenderPipelineState:pso];
    [renc drawPrimitives:MTLPrimitiveTypeTriangle vertexStart:0 vertexCount:3];
    [renc endEncoding];

    id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
    [blit copyFromTexture:target sourceSlice:0 sourceLevel:0
             sourceOrigin:MTLOriginMake(0, 0, 0) sourceSize:MTLSizeMake(W, H, 1)
                 toBuffer:readback destinationOffset:0 destinationBytesPerRow:W * 4 destinationBytesPerImage:W * H * 4];
    [blit endEncoding];

    [cb commit];
    [cb waitUntilCompleted];

    const uint8_t *px = (const uint8_t *)readback.contents;
    const uint8_t *center = px + (32 * W + 32) * 4;
    const uint8_t *corner = px + (2 * W + 2) * 4;
    int triPixels = 0, bluePixels = 0, otherPixels = 0;
    for (NSUInteger i = 0; i < W * H; i++) {
        const uint8_t *p = px + i * 4;
        if (p[0] > 240 && p[1] >= 45 && p[1] <= 57 && p[2] < 10 && p[3] == 255) triPixels++;
        else if (p[0] == 0 && p[1] == 0 && p[2] == 255 && p[3] == 255) bluePixels++;
        else otherPixels++;
    }
    printf("Render pass status=%lu: center=(%u,%u,%u,%u) corner=(%u,%u,%u,%u) triPx=%d bluePx=%d otherPx=%d\n",
           (unsigned long)cb.status,
           center[0], center[1], center[2], center[3],
           corner[0], corner[1], corner[2], corner[3],
           triPixels, bluePixels, otherPixels);

    bool ok = (cb.status == MTLCommandBufferStatusCompleted) &&
              (center[0] > 240 && center[2] < 10) &&
              (corner[0] == 0 && corner[2] == 255) &&
              (triPixels > 800 && bluePixels > 2000 && otherPixels == 0);
    printf("Render pass verdict: %s\n", ok ? "PASS" : "FAIL");
    return ok ? 0 : 1;
}

static bool render_ci_blur(id<MTLDevice> dev, const uint8_t *srcBytes, size_t W, size_t H, uint8_t *outBytes) {
    CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    NSData *srcData = [NSData dataWithBytes:srcBytes length:W * H * 4];
    CIImage *input = [CIImage imageWithBitmapData:srcData
                                      bytesPerRow:W * 4
                                             size:CGSizeMake(W, H)
                                           format:kCIFormatRGBA8
                                       colorSpace:cs];
    CIFilter *blur = [CIFilter filterWithName:@"CIGaussianBlur"];
    [blur setValue:input forKey:kCIInputImageKey];
    [blur setValue:@(4.0) forKey:kCIInputRadiusKey];
    CIImage *output = blur.outputImage;

    NSDictionary *opts = @{
        kCIContextWorkingFormat: @(kCIFormatRGBAh),
        kCIContextCacheIntermediates: @NO,
    };
    CIContext *ctx = [CIContext contextWithMTLDevice:dev options:opts];
    if (!ctx || !output) {
        CGColorSpaceRelease(cs);
        return false;
    }

    memset(outBytes, 0x55, W * H * 4);
    NSError *err = nil;
    CIRenderDestination *dest = [[CIRenderDestination alloc] initWithBitmapData:outBytes
                                                                          width:W
                                                                         height:H
                                                                    bytesPerRow:W * 4
                                                                         format:kCIFormatRGBA8];
    dest.colorSpace = cs;
    CIRenderTask *task = [ctx startTaskToRender:output
                                       fromRect:CGRectMake(0, 0, W, H)
                                  toDestination:dest
                                        atPoint:CGPointZero
                                          error:&err];
    bool ok = false;
    if (!task) {
        fprintf(stderr, "CoreImage(%s) startTaskToRender failed: %s\n",
                dev.name.UTF8String, err.localizedDescription.UTF8String ?: "(nil)");
    } else {
        CIRenderInfo *info = [task waitUntilCompletedAndReturnError:&err];
        if (!info) {
            fprintf(stderr, "CoreImage(%s) waitUntilCompleted failed: %s\n",
                    dev.name.UTF8String, err.localizedDescription.UTF8String ?: "(nil)");
        } else {
            ok = true;
        }
    }
    CGColorSpaceRelease(cs);
    return ok;
}

static int run_coreimage_blur_test(id<MTLDevice> cpuDev) {
    printf("\n=== [Test B] CoreImage CIContext(mtlDevice:) + CIGaussianBlur (CPU Metal vs Native GPU) ===\n");
    const size_t W = 64, H = 64;
    uint8_t srcBytes[W * H * 4];
    memset(srcBytes, 0, sizeof(srcBytes));
    // White 16x16 square in center [24..40), alpha=255 everywhere
    for (size_t y = 0; y < H; y++) {
        for (size_t x = 0; x < W; x++) {
            uint8_t *p = srcBytes + (y * W + x) * 4;
            bool inside = (x >= 24 && x < 40 && y >= 24 && y < 40);
            p[0] = inside ? 255 : 0;
            p[1] = inside ? 255 : 0;
            p[2] = inside ? 255 : 0;
            p[3] = 255;
        }
    }

    uint8_t cpuOut[W * H * 4];
    uint8_t gpuOut[W * H * 4];

    if (!render_ci_blur(cpuDev, srcBytes, W, H, cpuOut)) {
        printf("CoreImage CIGaussianBlur verdict: FAIL (CPU Metal render failed)\n");
        return 1;
    }

    id<MTLDevice> nativeDev = MTLCreateSystemDefaultDevice();
    if (!nativeDev || !render_ci_blur(nativeDev, srcBytes, W, H, gpuOut)) {
        printf("CoreImage CIGaussianBlur verdict: FAIL (Native GPU reference render failed)\n");
        return 1;
    }

    int maxErr = 0;
    double sumErr = 0.0;
    size_t nonZeroDiff = 0;
    for (size_t i = 0; i < W * H * 4; i++) {
        int d = abs((int)cpuOut[i] - (int)gpuOut[i]);
        if (d > maxErr) maxErr = d;
        if (d > 0) nonZeroDiff++;
        sumErr += (double)d;
    }
    double meanErr = sumErr / (double)(W * H * 4);

    const uint8_t *cc = cpuOut + (32 * W + 32) * 4;
    const uint8_t *ce = cpuOut + (22 * W + 32) * 4;
    const uint8_t *co = cpuOut + (2 * W + 2) * 4;
    const uint8_t *gc = gpuOut + (32 * W + 32) * 4;
    const uint8_t *ge = gpuOut + (22 * W + 32) * 4;
    const uint8_t *go = gpuOut + (2 * W + 2) * 4;

    printf("CPU Metal (%s): center=(%u,%u,%u,%u) edge_2px_out=(%u,%u,%u,%u) corner=(%u,%u,%u,%u)\n",
           cpuDev.name.UTF8String,
           cc[0], cc[1], cc[2], cc[3], ce[0], ce[1], ce[2], ce[3], co[0], co[1], co[2], co[3]);
    printf("Native GPU (%s): center=(%u,%u,%u,%u) edge_2px_out=(%u,%u,%u,%u) corner=(%u,%u,%u,%u)\n",
           nativeDev.name.UTF8String,
           gc[0], gc[1], gc[2], gc[3], ge[0], ge[1], ge[2], ge[3], go[0], go[1], go[2], go[3]);
    printf("Pixel diff (64x64x4=%zu channels): maxErr=%d, meanErr=%.4f, nonZeroDiff=%zu\n",
           W * H * 4, maxErr, meanErr, nonZeroDiff);

    bool ok = (cc[0] > 180) && (ce[0] > 20 && ce[0] < cc[0]) && (co[0] < 10) && (maxErr <= 20) && (meanErr < 4.0);
    printf("CoreImage CIGaussianBlur verdict: %s\n", ok ? "PASS" : "FAIL");
    return ok ? 0 : 1;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc < 3) {
            fprintf(stderr, "Usage: %s <XnuCpuMetal.bundle> <tri.metallib>\n", argv[0]);
            return 2;
        }
        NSBundle *bundle = [NSBundle bundleWithPath:[NSString stringWithUTF8String:argv[1]]];
        NSError *loadErr = nil;
        if (!bundle || ![bundle loadAndReturnError:&loadErr]) {
            fprintf(stderr, "Failed to load bundle: %s\n", loadErr.localizedDescription.UTF8String);
            return 1;
        }
        id<MTLDevice> dev = [[[bundle principalClass] alloc] initWithAcceleratorPort:0];
        if (!dev) {
            fprintf(stderr, "initWithAcceleratorPort returned nil\n");
            return 1;
        }
        printf("Loaded device: %s\n", dev.name.UTF8String);
        int rc1 = run_render_pass_test(dev, [NSString stringWithUTF8String:argv[2]]);
        int rc2 = run_coreimage_blur_test(dev);
        return (rc1 == 0 && rc2 == 0) ? 0 : 1;
    }
}

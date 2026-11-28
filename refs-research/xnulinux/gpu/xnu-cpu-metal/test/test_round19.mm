// SPDX-License-Identifier: GPL-2.0-or-later
// Round 19 regression test (non-fallback fixes):
// 1. MTLDevice linear/IOSurface texture alignment queries return 16 (matching Apple Silicon).
// 2. 2-plane 10-bit YCbCr IOSurface ('x420') -> MTLPixelFormatYCBCR10_420_2P (505) texture creation + shader sampling.
// 3. CoreImage CIGaussianBlur across non-64-byte-aligned widths & radii (reproduces iconservicesagent MPSImageGaussianBlur crash when alignment != 16).

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreImage/CoreImage.h>
#import <IOSurface/IOSurface.h>
#import <objc/message.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>

@interface NSObject (XnuCpuMetalPlugin)
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static id<MTLDevice> loadBundleDevice(const char *bundlePath) {
    NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
    if (!b || ![b load]) {
        fprintf(stderr, "FAIL: cannot load bundle %s\n", bundlePath);
        return nil;
    }
    Class cls = [b principalClass];
    if (!cls) {
        fprintf(stderr, "FAIL: no principalClass\n");
        return nil;
    }
    id<MTLDevice> dev = [[cls alloc] initWithAcceleratorPort:0];
    if (!dev) {
        fprintf(stderr, "FAIL: initWithAcceleratorPort:0 returned nil\n");
        return nil;
    }
    return dev;
}

static int testAlignmentQueries(id<MTLDevice> dev) {
    NSUInteger a1 = [dev minimumLinearTextureAlignmentForPixelFormat:MTLPixelFormatBGRA8Unorm];
    NSUInteger a2 = [dev minimumLinearTextureAlignmentForPixelFormat:MTLPixelFormatRGBA16Float];
    NSUInteger a3 = [dev minimumTextureBufferAlignmentForPixelFormat:MTLPixelFormatBGRA8Unorm];
    SEL selIso = NSSelectorFromString(@"iosurfaceTextureAlignmentBytes");
    SEL selLin = NSSelectorFromString(@"linearTextureAlignmentBytes");
    NSUInteger isoA = [dev respondsToSelector:selIso]
        ? ((NSUInteger(*)(id, SEL))objc_msgSend)(dev, selIso) : 0;
    NSUInteger linA = [dev respondsToSelector:selLin]
        ? ((NSUInteger(*)(id, SEL))objc_msgSend)(dev, selLin) : 0;
    printf("[1/3] Alignments: minLinear(BGRA8)=%lu minLinear(RGBA16F)=%lu minTexBuf=%lu iosurface=%lu linear=%lu\n",
           (unsigned long)a1, (unsigned long)a2, (unsigned long)a3, (unsigned long)isoA, (unsigned long)linA);
    if (a1 != 16 || a2 != 16 || a3 != 16 || isoA != 16 || linA != 16) {
        fprintf(stderr, "FAIL [1/3]: expected 16-byte alignment (Apple Silicon), got %lu/%lu/%lu/%lu/%lu\n",
                (unsigned long)a1, (unsigned long)a2, (unsigned long)a3, (unsigned long)isoA, (unsigned long)linA);
        return 1;
    }
    printf("PASS [1/3]: all linear/IOSurface texture alignments == 16\n");
    return 0;
}

static int testYCbCrIOSurface(id<MTLDevice> dev) {
    const int W = 64, H = 64;
    NSDictionary *plane0 = @{
        (id)kIOSurfacePlaneWidth: @(W),
        (id)kIOSurfacePlaneHeight: @(H),
        (id)kIOSurfacePlaneBytesPerElement: @(2),
        (id)kIOSurfacePlaneBytesPerRow: @(W * 2),
        (id)kIOSurfacePlaneSize: @(W * 2 * H),
        (id)kIOSurfacePlaneOffset: @(0),
    };
    NSDictionary *plane1 = @{
        (id)kIOSurfacePlaneWidth: @(W / 2),
        (id)kIOSurfacePlaneHeight: @(H / 2),
        (id)kIOSurfacePlaneBytesPerElement: @(4),
        (id)kIOSurfacePlaneBytesPerRow: @((W / 2) * 4),
        (id)kIOSurfacePlaneSize: @((W / 2) * 4 * (H / 2)),
        (id)kIOSurfacePlaneOffset: @(W * 2 * H),
    };
    NSDictionary *props = @{
        (id)kIOSurfaceWidth: @(W),
        (id)kIOSurfaceHeight: @(H),
        (id)kIOSurfacePixelFormat: @((uint32_t)'x420'),
        (id)kIOSurfaceAllocSize: @(W * 2 * H + (W / 2) * 4 * (H / 2)),
        (id)kIOSurfacePlaneInfo: @[plane0, plane1],
    };
    IOSurfaceRef surf = IOSurfaceCreate((__bridge CFDictionaryRef)props);
    if (!surf) {
        fprintf(stderr, "FAIL [2/3]: IOSurfaceCreate('x420') returned NULL\n");
        return 1;
    }
    IOSurfaceSetValue(surf, CFSTR("IOSurfaceYCbCrMatrix"), CFSTR("ITU_R_709_2"));

    IOSurfaceLock(surf, 0, NULL);
    uint16_t *yPlane = (uint16_t *)IOSurfaceGetBaseAddressOfPlane(surf, 0);
    size_t yBpr = IOSurfaceGetBytesPerRowOfPlane(surf, 0) / 2;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++)
            yPlane[y * yBpr + x] = (uint16_t)(940 << 6); // 10-bit video-range white Y
    uint16_t *uvPlane = (uint16_t *)IOSurfaceGetBaseAddressOfPlane(surf, 1);
    size_t uvBpr = IOSurfaceGetBytesPerRowOfPlane(surf, 1) / 2;
    for (int y = 0; y < H / 2; y++)
        for (int x = 0; x < W / 2; x++) {
            uvPlane[y * uvBpr + x * 2 + 0] = (uint16_t)(512 << 6); // neutral Cb
            uvPlane[y * uvBpr + x * 2 + 1] = (uint16_t)(512 << 6); // neutral Cr
        }
    IOSurfaceUnlock(surf, 0, NULL);

    MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:(MTLPixelFormat)505
                                                                                  width:W
                                                                                 height:H
                                                                              mipmapped:NO];
    td.usage = MTLTextureUsageShaderRead;
    id<MTLTexture> tex = [dev newTextureWithDescriptor:td iosurface:surf plane:0];
    if (!tex) {
        fprintf(stderr, "FAIL [2/3]: newTextureWithDescriptor:505 iosurface:'x420' returned nil\n");
        CFRelease(surf);
        return 1;
    }

    NSString *src = @"#include <metal_stdlib>\n"
                     "using namespace metal;\n"
                     "kernel void sample_ycbcr(texture2d<float, access::sample> t [[texture(0)]],\n"
                     "                         device float4 *out [[buffer(0)]],\n"
                     "                         uint gid [[thread_position_in_grid]]) {\n"
                     "    constexpr sampler s(filter::linear);\n"
                     "    if (gid == 0) out[0] = t.sample(s, float2(0.5, 0.5));\n"
                     "}\n";
    NSError *err = nil;
    id<MTLLibrary> lib = [dev newLibraryWithSource:src options:nil error:&err];
    id<MTLFunction> fn = [lib newFunctionWithName:@"sample_ycbcr"];
    id<MTLComputePipelineState> pso = [dev newComputePipelineStateWithFunction:fn error:&err];
    if (!pso) {
        fprintf(stderr, "FAIL [2/3]: compute PSO failed: %s\n", err.localizedDescription.UTF8String);
        CFRelease(surf);
        return 1;
    }

    id<MTLBuffer> outBuf = [dev newBufferWithLength:sizeof(float) * 4 options:MTLResourceStorageModeShared];
    id<MTLCommandQueue> q = [dev newCommandQueue];
    id<MTLCommandBuffer> cb = [q commandBuffer];
    id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
    [enc setComputePipelineState:pso];
    [enc setTexture:tex atIndex:0];
    [enc setBuffer:outBuf offset:0 atIndex:0];
    [enc dispatchThreads:MTLSizeMake(1, 1, 1) threadsPerThreadgroup:MTLSizeMake(1, 1, 1)];
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];

    float *rgba = (float *)outBuf.contents;
    printf("[2/3] Sampled YCBCR10_420_2P (505) white -> rgba=(%.3f, %.3f, %.3f, %.3f)\n",
           rgba[0], rgba[1], rgba[2], rgba[3]);
    CFRelease(surf);
    if (fabsf(rgba[0] - 1.0f) > 0.03f || fabsf(rgba[1] - 1.0f) > 0.03f ||
        fabsf(rgba[2] - 1.0f) > 0.03f || fabsf(rgba[3] - 1.0f) > 0.03f) {
        fprintf(stderr, "FAIL [2/3]: expected ~(1, 1, 1, 1)\n");
        return 1;
    }
    printf("PASS [2/3]: 2-plane YCBCR10_420_2P IOSurface sampled accurately\n");
    return 0;
}

static int testCIGaussianBlurNon64Aligned(id<MTLDevice> dev) {
    CIContext *ctx = [CIContext contextWithMTLDevice:dev];
    const int widths[] = {124, 126, 128, 130, 136, 140};
    const float radii[] = {6.0f, 10.0f, 15.0f};
    int cases = 0;
    for (int w : widths) {
        for (float r : radii) {
            CIImage *base = [[CIImage imageWithColor:[CIColor colorWithRed:0.8 green:0.4 blue:0.15 alpha:1.0]]
                imageByCroppingToRect:CGRectMake(20, 20, w - 40, w - 40)];
            CIFilter *blur = [CIFilter filterWithName:@"CIGaussianBlur"];
            [blur setValue:base forKey:kCIInputImageKey];
            [blur setValue:@(r) forKey:kCIInputRadiusKey];
            CIImage *out = [blur.outputImage imageByCroppingToRect:CGRectMake(0, 0, w, w)];
            uint8_t *buf = (uint8_t *)calloc((size_t)w * (size_t)w * 4, 1);
            [ctx render:out
               toBitmap:buf
               rowBytes:(size_t)w * 4
                 bounds:CGRectMake(0, 0, w, w)
                 format:kCIFormatRGBA8
             colorSpace:CGColorSpaceCreateDeviceRGB()];
            int cx = w / 2, cy = w / 2;
            uint8_t *p = buf + ((size_t)cy * (size_t)w + (size_t)cx) * 4;
            bool ok = (p[0] > 50 && p[3] > 50);
            free(buf);
            if (!ok) {
                fprintf(stderr, "FAIL [3/3]: CIGaussianBlur w=%d r=%.0f center=(%u,%u,%u,%u)\n",
                        w, r, p[0], p[1], p[2], p[3]);
                return 1;
            }
            cases++;
        }
    }
    printf("PASS [3/3]: CoreImage CIGaussianBlur passed all %d non-64-aligned width/radius combinations\n", cases);
    return 0;
}

int main(int argc, char **argv) {
    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : getenv("XNU_CPU_METAL_BUNDLE");
        if (!bundlePath) {
            fprintf(stderr, "Usage: %s <path/to/XnuCpuMetal.bundle>\n", argv[0]);
            return 2;
        }
        id<MTLDevice> dev = loadBundleDevice(bundlePath);
        if (!dev) return 1;
        printf("Device: %s\n", dev.name.UTF8String);
        int rc = 0;
        rc |= testAlignmentQueries(dev);
        rc |= testYCbCrIOSurface(dev);
        rc |= testCIGaussianBlurNon64Aligned(dev);
        if (rc == 0) printf("ALL ROUND 19 TESTS PASSED!\n");
        return rc;
    }
}

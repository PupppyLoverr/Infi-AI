// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <QuartzCore/QuartzCore.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vector>

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static id<MTLDevice> loadBundleDevice(const char *bundlePath) {
    NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
    if (!b || ![b load]) return nil;
    return (id<MTLDevice>)[(XcmBundleDeviceStub *)[b.principalClass alloc] initWithAcceleratorPort:0u];
}

static CGImageRef makeQuadrantImage(int w, int h) {
    CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef cg = CGBitmapContextCreate(NULL, w, h, 8, 0, cs,
                                            kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGFloat rgb[4][3] = {{1,0,0}, {0,1,0}, {0,0,1}, {1,1,0}};
    for (int i = 0; i < 4; i++) {
        CGContextSetRGBFillColor(cg, rgb[i][0], rgb[i][1], rgb[i][2], 1);
        CGContextFillRect(cg, CGRectMake((i % 2) * (w / 2), (i / 2) * (h / 2), w / 2, h / 2));
    }
    CGImageRef img = CGBitmapContextCreateImage(cg);
    CGContextRelease(cg);
    CGColorSpaceRelease(cs);
    return img;
}

static std::vector<uint8_t> renderCase(id<MTLDevice> dev, const char *mode, int W, int H) {
    MTLTextureDescriptor *td = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                  width:W height:H mipmapped:NO];
    td.storageMode = MTLStorageModeShared;
    td.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
    id<MTLTexture> tex = [dev newTextureWithDescriptor:td];

    CALayer *root = [CALayer layer];
    root.bounds = CGRectMake(0, 0, W, H);
    root.position = CGPointMake(W / 2.0, H / 2.0);
    root.geometryFlipped = NO;

    CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    if (!strcmp(mode, "color")) {
        CGFloat comps[4] = {1.0, 0.0, 0.0, 1.0};
        CGColorRef red = CGColorCreate(cs, comps);
        root.backgroundColor = red;
        CGColorRelease(red);
    } else if (!strcmp(mode, "image")) {
        CGImageRef img = makeQuadrantImage(W, H);
        root.contents = (__bridge id)img;
        CGImageRelease(img);
    } else if (!strcmp(mode, "two_layers")) {
        CGFloat bgc[4] = {0.2, 0.8, 0.2, 1.0};
        CGColorRef bg = CGColorCreate(cs, bgc);
        root.backgroundColor = bg;
        CGColorRelease(bg);
        CALayer *child = [CALayer layer];
        child.frame = CGRectMake(32, 32, W - 64, H - 64);
        CGFloat cc[4] = {0.2, 0.5, 0.9, 1.0};
        CGColorRef blue = CGColorCreate(cs, cc);
        child.backgroundColor = blue;
        CGColorRelease(blue);
        [root addSublayer:child];
    } else if (!strcmp(mode, "rounded_only")) {
        CGFloat bgc[4] = {0.9, 0.9, 0.95, 1.0};
        CGColorRef bg = CGColorCreate(cs, bgc);
        root.backgroundColor = bg;
        CGColorRelease(bg);
        CALayer *child = [CALayer layer];
        child.frame = CGRectMake(32, 32, W - 64, H - 64);
        CGFloat cc[4] = {0.2, 0.5, 0.9, 1.0};
        CGColorRef blue = CGColorCreate(cs, cc);
        child.backgroundColor = blue;
        CGColorRelease(blue);
        child.cornerRadius = 24.0;
        [root addSublayer:child];
    } else if (!strcmp(mode, "rounded_shadow")) {
        CGFloat bgc[4] = {0.9, 0.9, 0.95, 1.0};
        CGColorRef bg = CGColorCreate(cs, bgc);
        root.backgroundColor = bg;
        CGColorRelease(bg);

        CALayer *child = [CALayer layer];
        child.frame = CGRectMake(32, 32, W - 64, H - 64);
        CGFloat cc[4] = {0.2, 0.5, 0.9, 1.0};
        CGColorRef blue = CGColorCreate(cs, cc);
        child.backgroundColor = blue;
        CGColorRelease(blue);
        child.cornerRadius = 24.0;
        child.shadowOpacity = 0.8f;
        child.shadowRadius = 8.0;
        child.shadowOffset = CGSizeMake(0, 4);
        [root addSublayer:child];
    }
    CGColorSpaceRelease(cs);

    fprintf(stderr, "TEST %s dev=%s tex=%p\n", mode, dev.name.UTF8String, (__bridge void *)tex);
    CARenderer *renderer = [CARenderer rendererWithMTLTexture:tex options:nil];
    renderer.layer = root;
    renderer.bounds = CGRectMake(0, 0, W, H);
    [CATransaction flush];
    [renderer beginFrameAtTime:CACurrentMediaTime() timeStamp:NULL];
    [renderer addUpdateRect:renderer.bounds];
    [renderer render];
    [renderer endFrame];

    // Flush GPU queue to ensure CARenderer's internal CB completed
    id<MTLCommandQueue> q = [dev newCommandQueue];
    id<MTLCommandBuffer> cb = [q commandBuffer];
    [cb commit];
    [cb waitUntilCompleted];
    usleep(300000);

    std::vector<uint8_t> px(W * H * 4, 0);
    [tex getBytes:px.data() bytesPerRow:W * 4 fromRegion:MTLRegionMake2D(0, 0, W, H) mipmapLevel:0];
    return px;
}

static void printSummary(const char *tag, const char *mode, const std::vector<uint8_t> &px, int W, int H) {
    size_t nz = 0, nzRGB = 0;
    for (int i = 0; i < W * H; i++) {
        if (px[i*4+0] || px[i*4+1] || px[i*4+2]) nzRGB++;
        for (int c = 0; c < 4; c++) if (px[i*4+c]) nz++;
    }
    auto at = [&](int x, int y) {
        const uint8_t *p = &px[(y * W + x) * 4];
        printf("(%d,%d)=BGRA(%3u,%3u,%3u,%3u)", x, y, p[0], p[1], p[2], p[3]);
    };
    printf("[%s %-14s] nz=%zu/%zu nzRGB=%zu/%d  ", tag, mode, nz, px.size(), nzRGB, W * H);
    at(W/4, H/4); printf(" ");
    at(3*W/4, H/4); printf(" ");
    at(W/2, H/2); printf(" "); at(0, 0); printf("\n");
}

int main(int argc, char **argv) {
    setvbuf(stdout, nullptr, _IONBF, 0);
    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : "dist/XnuCpuMetal.bundle";
        id<MTLDevice> cpuDev = loadBundleDevice(bundlePath);
        id<MTLDevice> gpuDev = MTLCreateSystemDefaultDevice();
        if (!cpuDev || !gpuDev) { fprintf(stderr, "FAIL: devices %p %p\n", (__bridge void *)cpuDev, (__bridge void *)gpuDev); return 1; }
        printf("CPU: %s\nGPU: %s\n", cpuDev.name.UTF8String, gpuDev.name.UTF8String);

        const int W = 128, H = 128;
        const char *modes[] = {"color", "image", "two_layers", "rounded_only", "rounded_shadow"};
        int failures = 0;
        for (const char *m : modes) {
            auto ref = renderCase(gpuDev, m, W, H);
            auto got = renderCase(cpuDev, m, W, H);
            printSummary("GPU", m, ref, W, H);
            printSummary("CPU", m, got, W, H);
            int maxErr = 0;
            double sumErr = 0;
            for (size_t i = 0; i < ref.size(); i++) {
                int d = abs((int)ref[i] - (int)got[i]);
                if (d > maxErr) maxErr = d;
                sumErr += d;
            }
            double meanErr = sumErr / ref.size();
            printf("  DIFF %-14s: maxErr=%d meanErr=%.3f\n", m, maxErr, meanErr);
            if (maxErr > 8 || meanErr > 2.0) failures++;
        }
        return failures ? 1 : 0;
    }
}

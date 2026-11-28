// SPDX-License-Identifier: GPL-2.0-or-later
// Round 17 verification: deterministic direct driver loading (VK_LUNARG_direct_driver_loading)
// Verifies:
// 1. Selected backend loads ONLY its own driver dylib (unused ICDs in icd.d are never dlopen'd).
// 2. Hostile VK_ICD_FILENAMES / VK_DRIVER_FILES (or secure-mode ignoring env vars) cannot hijack backend.
// 3. Multi-ICD enumeration (lvp + MoltenVK simultaneously visible) picks non-CPU GPU over lavapipe.
// 4. lavapipe-only fallback & forced backend overrides work cleanly.

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <mach-o/dyld.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static bool isImageLoaded(const char *substr) {
    uint32_t n = _dyld_image_count();
    for (uint32_t i = 0; i < n; i++) {
        const char *name = _dyld_get_image_name(i);
        if (name && strstr(name, substr)) return true;
    }
    return false;
}

int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc < 4) {
            fprintf(stderr, "Usage: %s <bundle_path> <expected_name_substr|NIL> <forbidden_dylib_substr|NONE>\n", argv[0]);
            return 2;
        }
        const char *bundlePath = argv[1];
        const char *expectSub = argv[2];
        const char *forbidSub = argv[3];

        NSBundle *b = [NSBundle bundleWithPath:@(bundlePath)];
        if (!b || ![b load]) {
            fprintf(stderr, "FAIL: failed to load bundle %s\n", bundlePath);
            return 1;
        }
        Class cls = [b principalClass];
        if (!cls) {
            fprintf(stderr, "FAIL: no principalClass\n");
            return 1;
        }
        id<MTLDevice> dev = (id<MTLDevice>)[(XcmBundleDeviceStub *)[cls alloc] initWithAcceleratorPort:0u];

        bool expectNil = (strcmp(expectSub, "NIL") == 0);
        if (expectNil) {
            if (dev != nil) {
                fprintf(stderr, "FAIL: expected nil device, got '%s'\n", dev.name.UTF8String);
                return 1;
            }
            if (strcmp(forbidSub, "NONE") != 0 && isImageLoaded(forbidSub)) {
                fprintf(stderr, "FAIL: forbidden image '%s' was loaded!\n", forbidSub);
                return 1;
            }
            printf("PASS: device=nil (expected), forbidden='%s' loaded=0\n", forbidSub);
            return 0;
        }

        if (!dev) {
            fprintf(stderr, "FAIL: initWithAcceleratorPort: returned nil (expected '%s')\n", expectSub);
            return 1;
        }
        const char *devName = dev.name.UTF8String ?: "(null)";
        if (!strstr(devName, expectSub)) {
            fprintf(stderr, "FAIL: device.name='%s' does not contain '%s'\n", devName, expectSub);
            return 1;
        }
        if (strcmp(forbidSub, "NONE") != 0 && isImageLoaded(forbidSub)) {
            fprintf(stderr, "FAIL: forbidden image '%s' was loaded alongside '%s'!\n", forbidSub, devName);
            return 1;
        }

        // Smoke-test a real buffer + blit fill to confirm the selected backend executes cleanly.
        id<MTLBuffer> buf = [dev newBufferWithLength:256 options:MTLResourceStorageModeShared];
        id<MTLCommandQueue> q = [dev newCommandQueue];
        id<MTLCommandBuffer> cb = [q commandBuffer];
        id<MTLBlitCommandEncoder> blit = [cb blitCommandEncoder];
        [blit fillBuffer:buf range:NSMakeRange(0, 256) value:0x5A];
        [blit endEncoding];
        [cb commit];
        [cb waitUntilCompleted];
        if (cb.status != MTLCommandBufferStatusCompleted) {
            fprintf(stderr, "FAIL: blit fill status=%lu\n", (unsigned long)cb.status);
            return 1;
        }
        const uint8_t *p = (const uint8_t *)buf.contents;
        for (int i = 0; i < 256; i++) {
            if (p[i] != 0x5A) {
                fprintf(stderr, "FAIL: buf[%d]=0x%02x != 0x5A\n", i, p[i]);
                return 1;
            }
        }
        printf("PASS: device='%s', forbidden='%s' loaded=0, blitFill=OK\n", devName, forbidSub);
        return 0;
    }
}

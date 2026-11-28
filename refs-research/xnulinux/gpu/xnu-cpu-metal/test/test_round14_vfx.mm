// SPDX-License-Identifier: GPL-2.0-or-later
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOKit/IOKitLib.h>
#import <AppKit/AppKit.h>
#import <SceneKit/SceneKit.h>
#import <objc/runtime.h>
#import <objc/message.h>
#include <dlfcn.h>
#include <execinfo.h>
#include <mach-o/dyld.h>
#include <mach-o/getsect.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <set>
#include <string>

static void crashHandler(int sig) {
    void *bt[40];
    int n = backtrace(bt, 40);
    fprintf(stderr, "\n=== CRASH sig=%d ===\n", sig);
    backtrace_symbols_fd(bt, n, 2);
    _exit(128 + sig);
}

@interface XcmBundleDeviceStub : NSObject
- (id)initWithAcceleratorPort:(unsigned)port;
@end

static void collectProtocolSels(Protocol *p, std::set<std::string> &out, std::set<Protocol *> &seen) {
    if (!p || seen.count(p)) return;
    seen.insert(p);
    for (int req = 0; req <= 1; req++) {
        unsigned cnt = 0;
        struct objc_method_description *mds = protocol_copyMethodDescriptionList(p, req != 0, YES, &cnt);
        for (unsigned i = 0; i < cnt; i++) {
            if (mds[i].name) out.insert(sel_getName(mds[i].name));
        }
        free(mds);
    }
    unsigned pcnt = 0;
    Protocol * __unsafe_unretained *plist = protocol_copyProtocolList(p, &pcnt);
    for (unsigned i = 0; i < pcnt; i++) collectProtocolSels(plist[i], out, seen);
    free(plist);
}

int main(int argc, char **argv) {
    signal(SIGSEGV, crashHandler);
    signal(SIGBUS, crashHandler);
    signal(SIGABRT, crashHandler);
    setvbuf(stdout, nullptr, _IONBF, 0);
    @autoreleasepool {
        const char *bundlePath = (argc > 1) ? argv[1] : "dist/XnuCpuMetal.bundle";
        id<MTLDevice> native = MTLCreateSystemDefaultDevice();
        NSBundle *b = [NSBundle bundleWithPath:[NSString stringWithUTF8String:bundlePath]];
        if (!b || ![b load]) { fprintf(stderr, "FAIL: load bundle\n"); return 1; }
        // Pass a real IOAccelerator service port so XcmBackendDevice gets a valid IORegistry registryID
        // (matching hosted macOS; VFX looks up devices by registryID in its global resource manager table).
        io_service_t accelPort = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOAccelerator"));
        id<MTLDevice> dev = (id<MTLDevice>)[(XcmBundleDeviceStub *)[b.principalClass alloc] initWithAcceleratorPort:accelPort];
        if (accelPort) IOObjectRelease(accelPort);
        if (!dev) { fprintf(stderr, "FAIL: device nil\n"); return 2; }
        printf("Round 14 test: %s\n", dev.name.UTF8String);

        // [1] Check isDepth24Stencil8PixelFormatSupported == NO (matches Apple Silicon)
        if ([dev isDepth24Stencil8PixelFormatSupported] != NO) {
            fprintf(stderr, "FAIL [1]: isDepth24Stencil8PixelFormatSupported expected NO\n");
            return 3;
        }
        printf("PASS [1]: isDepth24Stencil8PixelFormatSupported == NO\n");

        // [2] Protocol completeness vs Apple Silicon native device (MTLDevice + MTLDeviceSPI)
        std::set<std::string> allSels;
        std::set<Protocol *> seen;
        collectProtocolSels(@protocol(MTLDevice), allSels, seen);
        collectProtocolSels(objc_getProtocol("MTLDeviceSPI"), allSels, seen);
        int missingProto = 0;
        for (const auto &s : allSels) {
            SEL sel = sel_registerName(s.c_str());
            if ([native respondsToSelector:sel] && ![dev respondsToSelector:sel]) {
                fprintf(stderr, "FAIL [2]: missing protocol selector: %s\n", s.c_str());
                missingProto++;
            }
        }
        if (missingProto) return 4;
        printf("PASS [2]: 0 missing selectors across MTLDevice + MTLDeviceSPI\n");

        // [3] Depth32Float_Stencil8 render pass verification (VFX fallback depth format)
        {
            MTLTextureDescriptor *ctd = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                                                                           width:64 height:64 mipmapped:NO];
            ctd.storageMode = MTLStorageModeShared;
            ctd.usage = MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead;
            id<MTLTexture> colorTex = [dev newTextureWithDescriptor:ctd];

            MTLTextureDescriptor *dtd = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatDepth32Float_Stencil8
                                                                                           width:64 height:64 mipmapped:NO];
            dtd.storageMode = MTLStorageModePrivate;
            dtd.usage = MTLTextureUsageRenderTarget;
            id<MTLTexture> depthTex = [dev newTextureWithDescriptor:dtd];
            if (!colorTex || !depthTex) { fprintf(stderr, "FAIL [3]: Depth32Float_Stencil8 texture nil\n"); return 5; }

            MTLDepthStencilDescriptor *dsd = [[MTLDepthStencilDescriptor alloc] init];
            dsd.depthCompareFunction = MTLCompareFunctionLess;
            dsd.depthWriteEnabled = YES;
            id<MTLDepthStencilState> ds = [dev newDepthStencilStateWithDescriptor:dsd];
            if (!ds) { fprintf(stderr, "FAIL [3]: depthStencilState nil\n"); return 5; }

            id<MTLCommandQueue> q = [dev newCommandQueue];
            id<MTLCommandBuffer> cb = [q commandBuffer];
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = colorTex;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(0.2, 0.4, 0.6, 1.0);
            rp.depthAttachment.texture = depthTex;
            rp.depthAttachment.loadAction = MTLLoadActionClear;
            rp.depthAttachment.storeAction = MTLStoreActionDontCare;
            rp.depthAttachment.clearDepth = 1.0;
            rp.stencilAttachment.texture = depthTex;
            rp.stencilAttachment.loadAction = MTLLoadActionClear;
            rp.stencilAttachment.storeAction = MTLStoreActionDontCare;
            rp.stencilAttachment.clearStencil = 0;
            id<MTLRenderCommandEncoder> enc = [cb renderCommandEncoderWithDescriptor:rp];
            [enc setDepthStencilState:ds];
            [enc endEncoding];
            [cb commit];
            [cb waitUntilCompleted];
            if (cb.status != MTLCommandBufferStatusCompleted) {
                fprintf(stderr, "FAIL [3]: Depth32Float_Stencil8 render pass status=%lu\n", (unsigned long)cb.status);
                return 5;
            }
            printf("PASS [3]: Depth32Float_Stencil8 texture + depth/stencil render pass completed\n");
        }

        // [4] SceneKit SCNRenderer with our CPU Metal device (exercises SceneKit + VFX Metal device queries)
        {
            SCNRenderer *scn = [SCNRenderer rendererWithDevice:dev options:nil];
            if (!scn) { fprintf(stderr, "FAIL [4]: SCNRenderer nil\n"); return 6; }
            SCNScene *scene = [SCNScene scene];
            SCNNode *boxNode = [SCNNode nodeWithGeometry:[SCNBox boxWithWidth:1 height:1 length:1 chamferRadius:0.1]];
            [scene.rootNode addChildNode:boxNode];
            scn.scene = scene;
            printf("PASS [4]: SCNRenderer initialized with XcmBackendDevice (0 unrecognized selectors)\n");
        }

        // [5] VFX + AvatarKit AVTView initialization with our CPU Metal device
        {
            // Register dev in Metal.framework's device list (exact same registration IOKit performs
            // in hosted macOS; VFX looks up devices in MTLCopyAllDevices by registryID).
            void (*mtlAddDevice)(id<MTLDevice>) = (void (*)(id<MTLDevice>))dlsym(RTLD_DEFAULT, "MTLAddDevice");
            if (mtlAddDevice) mtlAddDevice(dev);

            void *vfxH = dlopen("/System/Library/PrivateFrameworks/VFX.framework/VFX", RTLD_NOW);
            void *avtH = dlopen("/System/Library/PrivateFrameworks/AvatarKit.framework/AvatarKit", RTLD_NOW);
            printf("dlopen VFX=%p AvatarKit=%p (MTLAddDevice=%p)\n", vfxH, avtH, (void *)mtlAddDevice);

            // Try VFXRenderer if present
            Class vfxRendererCls = objc_getClass("VFXRenderer");
            if (vfxRendererCls && [vfxRendererCls respondsToSelector:@selector(rendererWithDevice:options:)]) {
                id r = ((id (*)(id, SEL, id, id))objc_msgSend)(vfxRendererCls, @selector(rendererWithDevice:options:), dev, nil);
                printf("  VFXRenderer with XcmBackendDevice -> %p\n", (__bridge void *)r);
                if (!r) { fprintf(stderr, "FAIL [5]: VFXRenderer returned nil\n"); return 6; }
            }

            // Try AVTView initWithFrame:options: (exact loginwindow lock-screen path!)
            Class avtViewCls = objc_getClass("AVTView");
            if (avtViewCls) {
                NSDictionary *opts = @{@"AVTViewMetalDeviceKey": dev, @"SCNPreferredDeviceKey": dev};
                id view = [[avtViewCls alloc] initWithFrame:NSMakeRect(0, 0, 128, 128) options:opts];
                printf("  AVTView initWithFrame:options: -> %p\n", (__bridge void *)view);
                if (!view) { fprintf(stderr, "FAIL [5]: AVTView returned nil\n"); return 6; }
            }
            printf("PASS [5]: VFX + AvatarKit AVTView init completed with 0 unrecognized selectors\n");
        }

        // [6] Verify 0 missing __objc_selrefs across VFX, AvatarKit, SceneKit, CoreRE, QuartzCore, RenderBox, CoreImage
        {
            dlopen("/System/Library/PrivateFrameworks/CoreRE.framework/CoreRE", RTLD_LAZY);
            dlopen("/System/Library/Frameworks/QuartzCore.framework/QuartzCore", RTLD_LAZY);
            dlopen("/System/Library/PrivateFrameworks/RenderBox.framework/RenderBox", RTLD_LAZY);
            dlopen("/System/Library/Frameworks/CoreImage.framework/CoreImage", RTLD_LAZY);
            int missFw = 0;
            for (uint32_t i = 0; i < _dyld_image_count(); i++) {
                const char *name = _dyld_get_image_name(i);
                if (!name) continue;
                if (!strstr(name, "/VFX") && !strstr(name, "/AvatarKit") && !strstr(name, "/SceneKit") &&
                    !strstr(name, "/CoreRE") && !strstr(name, "/QuartzCore") && !strstr(name, "/RenderBox") &&
                    !strstr(name, "/CoreImage")) continue;
                const struct mach_header_64 *mh = (const struct mach_header_64 *)_dyld_get_image_header(i);
                const char *segs[] = {"__DATA_CONST", "__DATA", "__AUTH_CONST"};
                for (auto seg : segs) {
                    unsigned long sz = 0;
                    const SEL *sels = (const SEL *)getsectiondata(mh, seg, "__objc_selrefs", &sz);
                    if (!sels || !sz) continue;
                    size_t n = sz / sizeof(SEL);
                    for (size_t k = 0; k < n; k++) {
                        SEL sel = sels[k];
                        if (sel && [native respondsToSelector:sel] && ![dev respondsToSelector:sel]) {
                            fprintf(stderr, "FAIL [6]: missing framework selref %s in %s\n", sel_getName(sel), name);
                            missFw++;
                        }
                    }
                }
            }
            if (missFw) return 7;
            printf("PASS [6]: 0 missing __objc_selrefs across all 7 frameworks\n");
        }

        return 0;
    }
}

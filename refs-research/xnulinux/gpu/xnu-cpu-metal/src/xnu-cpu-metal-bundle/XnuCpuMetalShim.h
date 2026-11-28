// SPDX-License-Identifier: BSD-2-Clause-Patent
// Adapted from UniXNU/AdrenoMetal's IOGPUMetalDeviceShim.h (BSD-2-Clause-Patent).
// Installs macOS IOGPUMetalDevice compatibility selectors and Metal protocol conformances
// on the dynamically created _MTLDevice subclass and Xcm* frontend classes.
#pragma once
#import <Foundation/Foundation.h>
#include <CommonCrypto/CommonDigest.h>
#include <IOKit/IOKitLib.h>
#include <atomic>
#include <objc/runtime.h>
#include <stdio.h>
#include <string.h>

namespace xcm_iogpu {

inline io_service_t gAcceleratorPort;
inline id gAcceleratorService;
inline __weak id gDeviceWrapper;

static void setAcceleratorService(id, SEL, id service) { gAcceleratorService = service; }
static io_service_t acceleratorPort(id, SEL) { return gAcceleratorPort; }
static void setDeviceWrapper(id, SEL, id wrapper) { gDeviceWrapper = wrapper; }
static id deviceWrapper(id self, SEL) { id w = gDeviceWrapper; return w ? w : self; }
static BOOL isBuiltIn(id, SEL) { return YES; }
static BOOL no(id, SEL) { return NO; }
static uint64_t zero(id, SEL) { return 0; }
static uint32_t one(id, SEL) { return 1; }
static void nothing(id, SEL) {}
static void nothingWith(id, SEL, uint64_t) {}
static BOOL amplificationCount(id, SEL, NSUInteger count) { return count == 1; }
static uint64_t commandBufferArgumentsSize(id, SEL) { return 48; }
static uint64_t workingSetSize(id, SEL) { return NSProcessInfo.processInfo.physicalMemory / 3 * 2; }
static NSUInteger gpuCoreCount(id, SEL) { return 4; }
static uint64_t stub(id, SEL) { return 0; }

static Class referenceClass(Class cls) {
    static const struct { const char *frontend, *iogpu; } classes[] = {
        {"XcmBuffer", "IOGPUMetalBuffer"},
        {"XcmTexture", "IOGPUMetalTexture"},
        {"XcmQueue", "IOGPUMetalCommandQueue"},
        {"XcmCommandBuffer", "IOGPUMetalCommandBuffer"},
        {"XcmComputeEncoder", "IOGPUMetalComputeCommandEncoder"},
        {"XcmRenderEncoder", "IOGPUMetalRenderCommandEncoder"},
        {"XcmBlitEncoder", "IOGPUMetalBlitCommandEncoder"},
        {"XcmFence", "IOGPUMetalFence"},
        {"XcmEvent", "IOGPUMetalEvent"},
        {"XcmSharedEvent", "IOGPUMetalSharedEvent"},
        {"XcmHeap", "IOGPUMetalHeap"},
        {"XcmLibrary", "_MTLLibrary"},
        {"XcmFunction", "_MTLFunction"},
        {"XcmArgumentEncoder", "_MTLArgumentEncoder"},
        {"XcmComputePipeline", "_MTLComputePipelineState"},
        {"XcmRenderPipeline", "_MTLRenderPipelineState"},
        {"XcmObject", nullptr},
    };
    for (Class c = cls; c; c = class_getSuperclass(c))
        for (const auto &entry : classes)
            if (!strcmp(class_getName(c), entry.frontend))
                return entry.iogpu ? objc_getClass(entry.iogpu) : nil;
    return objc_getClass("IOGPUMetalDevice");
}

static BOOL resolveStub(id cls, SEL, SEL selector) {
    Class ref = referenceClass((Class)cls);
    Method method = ref ? class_getInstanceMethod(ref, selector) : nullptr;
    if (!method) return NO;
    fprintf(stderr, "XnuCpuMetal: %s: stubbed %s %s\n", getprogname(), class_getName((Class)cls), sel_getName(selector));
    return class_addMethod((Class)cls, selector, (IMP)stub, method_getTypeEncoding(method));
}

static char kPriorityKey, kBackgroundPriorityKey;
static NSUInteger priorityOf(id queue, const void *key, NSUInteger initial) {
    NSNumber *v = objc_getAssociatedObject(queue, key);
    return v ? v.unsignedIntegerValue : initial;
}
static NSUInteger getGPUPriority(id self, SEL) { return priorityOf(self, &kPriorityKey, 1); }
static NSUInteger getBackgroundGPUPriority(id self, SEL) { return priorityOf(self, &kBackgroundPriorityKey, 2); }
static void setPriorities(id self, NSUInteger priority, NSUInteger background) {
    objc_setAssociatedObject(self, &kPriorityKey, @(priority), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    objc_setAssociatedObject(self, &kBackgroundPriorityKey, @(background), OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}
static BOOL setGPUPriority(id self, SEL, NSUInteger priority) {
    setPriorities(self, priority, getBackgroundGPUPriority(self, nullptr));
    return YES;
}
static BOOL setGPUPriorityOffset(id self, SEL, NSUInteger priority, NSUInteger) { return setGPUPriority(self, nullptr, priority); }
static BOOL setBothGPUPriorities(id self, SEL, NSUInteger priority, NSUInteger background) {
    setPriorities(self, priority, background);
    return YES;
}

static char kUserDictionaryKey;
static id userDictionary(id self, SEL) {
    @synchronized(self) {
        NSMutableDictionary *d = objc_getAssociatedObject(self, &kUserDictionaryKey);
        if (!d) {
            d = [NSMutableDictionary dictionary];
            objc_setAssociatedObject(self, &kUserDictionaryKey, d, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        }
        return d;
    }
}

static char kBitCodeHashKey;
static const void *bitCodeHash(id self, SEL) {
    @synchronized(self) {
        NSData *hash = objc_getAssociatedObject(self, &kBitCodeHashKey);
        if (!hash) {
            static std::atomic<uint64_t> serial;
            NSString *text = [NSString stringWithFormat:@"%@#%llu", [self valueForKey:@"name"], (unsigned long long)++serial];
            NSData *bytes = [text dataUsingEncoding:NSUTF8StringEncoding];
            NSMutableData *digest = [NSMutableData dataWithLength:CC_SHA256_DIGEST_LENGTH];
            CC_SHA256(bytes.bytes, (CC_LONG)bytes.length, (unsigned char *)digest.mutableBytes);
            hash = digest;
            objc_setAssociatedObject(self, &kBitCodeHashKey, hash, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        }
        return hash.bytes;
    }
}

struct MethodSpec {
    const char *name;
    IMP imp;
    const char *types;
    bool overrideExisting;
};

static void addMethods(Class cls, const MethodSpec *methods, size_t count) {
    for (size_t i = 0; i < count; i++) {
        SEL sel = sel_registerName(methods[i].name);
        if (methods[i].overrideExisting || ![cls instancesRespondToSelector:sel])
            class_addMethod(cls, sel, methods[i].imp, methods[i].types);
    }
}

static void install(id device, io_service_t port) {
    if (port) IOObjectRetain(port);
    gAcceleratorPort = port;
    static const MethodSpec deviceMethods[] = {
        {"_setAcceleratorService:", (IMP)setAcceleratorService, "v24@0:8@16", false},
        {"acceleratorPort", (IMP)acceleratorPort, "I16@0:8", true},
        {"_setDeviceWrapper:", (IMP)setDeviceWrapper, "v24@0:8@16", true},
        {"_deviceWrapper", (IMP)deviceWrapper, "@16@0:8", true},
        {"isBuiltIn", (IMP)isBuiltIn, "B16@0:8", false},
        {"supportPriorityBand", (IMP)no, "B16@0:8", false},
        {"supportsBackgroundAppRole", (IMP)no, "B16@0:8", false},
        {"isSlotted", (IMP)no, "B16@0:8", false},
        {"supportsResourceDetachBacking", (IMP)no, "B16@0:8", false},
        {"supportsVertexAmplification", (IMP)no, "B16@0:8", false},
        {"supportsVertexAmplificationCount:", (IMP)amplificationCount, "B24@0:8Q16", false},
        {"maxTransferRate", (IMP)zero, "Q16@0:8", false},
        {"location", (IMP)zero, "Q16@0:8", false},
        {"locationNumber", (IMP)zero, "Q16@0:8", false},
        {"peerGroupID", (IMP)zero, "Q16@0:8", false},
        {"peerIndex", (IMP)zero, "I16@0:8", false},
        {"peerCount", (IMP)one, "I16@0:8", false},
        {"dedicatedMemorySize", (IMP)zero, "Q16@0:8", false},
        {"hwResourcePoolCount", (IMP)zero, "I16@0:8", false},
        {"cmdBufArgsSize", (IMP)commandBufferArgumentsSize, "Q16@0:8", false},
        {"kickCleanupQueue", (IMP)nothing, "v16@0:8", false},
        {"updateGPUSelectionProperties", (IMP)nothing, "v16@0:8", false},
        {"updateResourcePoolPurgeability", (IMP)nothing, "v16@0:8", false},
        {"periodicUpdateResourcePoolPurgeability", (IMP)nothing, "v16@0:8", false},
        {"cancelPeriodicUpdateResourcePoolPurgeability", (IMP)nothing, "v16@0:8", false},
        {"recommendedMaxWorkingSetSize", (IMP)workingSetSize, "Q16@0:8", true},
        {"sharedMemorySize", (IMP)workingSetSize, "Q16@0:8", true},
        {"currentAllocatedSize", (IMP)zero, "Q16@0:8", false},
        {"gpuCoreCount", (IMP)gpuCoreCount, "Q16@0:8", false},
        {"featureProfile", (IMP)zero, "Q16@0:8", false},
        {"compilerPropagatesThreadPriority:", (IMP)nothingWith, "v20@0:8B16", false},
    };
    Class cls = object_getClass(device);
    addMethods(cls, deviceMethods, sizeof deviceMethods / sizeof *deviceMethods);
    class_addMethod(object_getClass(cls), @selector(resolveInstanceMethod:), (IMP)resolveStub, "B24@0:8:16");

    static const MethodSpec queueMethods[] = {
        {"getGPUPriority", (IMP)getGPUPriority, "Q16@0:8", false},
        {"getBackgroundGPUPriority", (IMP)getBackgroundGPUPriority, "Q16@0:8", false},
        {"setGPUPriority:", (IMP)setGPUPriority, "B24@0:8Q16", false},
        {"setGPUPriority:offset:", (IMP)setGPUPriorityOffset, "B32@0:8Q16Q24", false},
        {"_setGPUPriority:backgroundPriority:", (IMP)setBothGPUPriorities, "B32@0:8Q16Q24", false},
        {"setCompletionQueue:", (IMP)nothingWith, "v24@0:8@16", false},
        {"setSubmissionQueue:", (IMP)nothingWith, "v24@0:8@16", false},
    };
    if (Class q = objc_getClass("XcmQueue"))
        addMethods(q, queueMethods, sizeof queueMethods / sizeof *queueMethods);

    static const MethodSpec resMethods[] = {
        {"setResponsibleProcess:", (IMP)nothingWith, "v20@0:8i16", false},
        {"makeAliasable", (IMP)nothing, "v16@0:8", false},
    };
    for (const char *name : {"XcmBuffer", "XcmTexture"})
        if (Class r = objc_getClass(name))
            addMethods(r, resMethods, sizeof resMethods / sizeof *resMethods);

    static const MethodSpec cbMethods[] = {{"userDictionary", (IMP)userDictionary, "@16@0:8", true}};
    static const MethodSpec fnMethods[] = {{"bitCodeHash", (IMP)bitCodeHash, "^v16@0:8", false}};
    if (Class fn = objc_getClass("XcmFunction")) addMethods(fn, fnMethods, 1);
    if (Class cb = objc_getClass("XcmCommandBuffer")) addMethods(cb, cbMethods, 1);

    static const char *const conformances[][2] = {
        {"XcmBuffer", "MTLBuffer"}, {"XcmTexture", "MTLTexture"},
        {"XcmLibrary", "MTLLibrary"}, {"XcmFunction", "MTLFunction"},
        {"XcmArgumentEncoder", "MTLArgumentEncoder"},
        {"XcmComputePipeline", "MTLComputePipelineState"},
        {"XcmRenderPipeline", "MTLRenderPipelineState"},
        {"XcmSampler", "MTLSamplerState"}, {"XcmDepthStencil", "MTLDepthStencilState"},
        {"XcmEvent", "MTLEvent"}, {"XcmSharedEvent", "MTLEvent"}, {"XcmSharedEvent", "MTLSharedEvent"}, {"XcmFence", "MTLFence"},
        {"XcmHeap", "MTLHeap"},
        {"XcmQueue", "MTLCommandQueue"}, {"XcmCommandBuffer", "MTLCommandBuffer"},
        {"XcmComputeEncoder", "MTLComputeCommandEncoder"},
        {"XcmRenderEncoder", "MTLRenderCommandEncoder"},
        {"XcmBlitEncoder", "MTLBlitCommandEncoder"},
    };
    for (const auto &c : conformances) {
        Class frontend = objc_getClass(c[0]);
        Protocol *proto = objc_getProtocol(c[1]);
        if (frontend && proto) class_addProtocol(frontend, proto);
    }
    if (Class obj = objc_getClass("XcmObject"))
        class_addMethod(object_getClass(obj), @selector(resolveInstanceMethod:), (IMP)resolveStub, "B24@0:8:16");
}

}  // namespace xcm_iogpu

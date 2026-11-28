// SPDX-License-Identifier: GPL-2.0-or-later
#include <os/lock.h>
#include <objc/message.h>
// XnuCpuMetal.bundle — Pure-CPU Metal plugin for xnulinux (macOS 27).
// Principal class: XnuCpuMetalDevice (subclass of IOGPUMetalDevice).
// Backend: libxnu_m2v.dylib (steelbrain/metal2vulkan) -> libvulkan.1.dylib -> Mesa lavapipe (libvulkan_lvp.dylib).

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <IOSurface/IOSurfaceRef.h>
#import <IOKit/IOKitLib.h>
#import <objc/runtime.h>
#import <objc/message.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <os/log.h>
#include <mach-o/dyld.h>
#include <mach-o/getsect.h>
#include <mach-o/loader.h>
#include <atomic>
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysctl.h>
#include <unistd.h>
#include <deque>
#include <functional>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

#include "xnu_m2v.h"
#include "XnuCpuMetalShim.h"

@interface IOGPUMetalDevice : NSObject
@end

@protocol XcmOriginalStitching
- (NSString *)newDagStringWithGraphs:(NSArray<MTLFunctionStitchingGraph *> *)graphs;
@end

@interface MTLFunctionConstantValues (XcmPrivateAccess)
- (NSArray *)newIndexedConstantArray;
- (NSArray *)newNamedConstantArray;
@end

@protocol XcmPrivateConstantValue
- (NSUInteger)dataType;
- (void *)data;
- (NSUInteger)index;
- (NSString *)name;
@end

static bool xcmTraceOn() { static bool on = getenv("XNU_CPU_METAL_TRACE") != nullptr; return on; }

static NSError *xcmError(NSString *domain, NSInteger code, const char *fallback) {
    const char *msg = xm2v_last_error();
    NSString *desc = (msg && msg[0]) ? [NSString stringWithUTF8String:msg]
                                     : [NSString stringWithUTF8String:(fallback ?: "XnuCpuMetal error")];
    return [NSError errorWithDomain:domain code:code userInfo:@{NSLocalizedDescriptionKey: desc}];
}

static uint64_t xcmConstantBytes(uint64_t type) {
    switch (type) {
        case 3: case 29: case 33: return 4;
        case 4: case 30: case 34: return 8;
        case 5: case 31: case 35: return 12;
        case 6: case 32: case 36: return 16;
        case 16: case 37: case 41: case 121: return 2;
        case 17: case 38: case 42: case 122: return 4;
        case 18: case 39: case 43: case 123: return 6;
        case 19: case 40: case 44: case 124: return 8;
        case 45: case 49: case 53: return 1;
        case 46: case 50: case 54: return 2;
        case 47: case 51: case 55: return 3;
        case 48: case 52: case 56: return 4;
        case 81: case 85: return 8;
        case 82: case 86: return 16;
        case 83: case 87: return 24;
        case 84: case 88: return 32;
        default: return 0;
    }
}

/* ======================== MTLCompiler CodeGen (Stitched Libraries) ======================== */

typedef void (^XcmCodeGenReply)(int32_t status, const void *reply, size_t length, const char *message);

static void *xcmRunBlock(void *block) {
    @autoreleasepool { ((__bridge void (^)(void))block)(); }
    return nullptr;
}

static void xcmOnLargeStack(void (^body)(void)) {
    pthread_attr_t attr;
    pthread_t thread;
    if (pthread_attr_init(&attr)) { body(); return; }
    bool made = !pthread_attr_setstacksize(&attr, 16u << 20) &&
                !pthread_create(&thread, &attr, xcmRunBlock, (__bridge void *)body);
    pthread_attr_destroy(&attr);
    if (made) pthread_join(thread, nullptr);
    else body();
}

static NSData *xcmCodeGenRequest(uint32_t type, const std::vector<uint8_t> &request,
                                 NSString **warnings, NSString **failure) {
    static void *service = nullptr;
    static void (*build)(void *, void *, uint32_t, const void *, size_t, XcmCodeGenReply) = nullptr;
    static dispatch_once_t once;
    static std::mutex lock;
    dispatch_once(&once, ^{
        static const char kFramework[] = "/System/Library/PrivateFrameworks/MTLCompiler.framework";
        std::string path = std::string(kFramework) + "/MTLCompiler";
        void *lib = dlopen(path.c_str(), RTLD_NOW);
        if (!lib) {
            static const char kPrefix[] = "/System/Library/PrivateFrameworks/MTLCompiler.framework/Versions/";
            for (uint32_t i = 0, n = _dyld_image_count(); i < n && !lib; i++) {
                const char *name = _dyld_get_image_name(i);
                if (!name || !strstr(name, "/Metal.framework/") || strcmp(strrchr(name, '/'), "/Metal")) continue;
                unsigned long size = 0;
                const char *strings = (const char *)getsectiondata(
                    (const struct mach_header_64 *)_dyld_get_image_header(i), "__TEXT", "__cstring", &size);
                for (unsigned long at = 0; strings && at + sizeof(kPrefix) < size;) {
                    const char *text = strings + at;
                    size_t len = strnlen(text, size - at);
                    if (!strncmp(text, kPrefix, sizeof(kPrefix) - 1) && len > sizeof(kPrefix) &&
                        !strcmp(text + len - 12, "/MTLCompiler")) {
                        path.assign(text, len);
                        lib = dlopen(path.c_str(), RTLD_NOW);
                        break;
                    }
                    at += len + 1;
                }
            }
        }
        if (!lib) {
            path = std::string(kFramework) + "/Versions/Current/MTLCompiler";
            lib = dlopen(path.c_str(), RTLD_NOW);
        }
        if (!lib) {
            unsigned long highest = 0;
            for (NSString *entry in [[NSFileManager defaultManager]
                     contentsOfDirectoryAtPath:[@(kFramework) stringByAppendingString:@"/Versions"] error:nil]) {
                unsigned long ver = strtoul(entry.UTF8String, nullptr, 10);
                if (ver > highest && [entry isEqualToString:[NSString stringWithFormat:@"%lu", ver]]) highest = ver;
            }
            if (highest) {
                path = std::string(kFramework) + "/Versions/" + std::to_string(highest) + "/MTLCompiler";
                lib = dlopen(path.c_str(), RTLD_NOW);
            }
        }
        auto create = lib ? (void *(*)(const char *))dlsym(lib, "MTLCodeGenServiceCreate") : nullptr;
        build = lib ? (void (*)(void *, void *, uint32_t, const void *, size_t, XcmCodeGenReply))
                          dlsym(lib, "MTLCodeGenServiceBuildRequest")
                    : nullptr;
        service = (create && build) ? create("xnu-cpu-metal") : nullptr;
    });
    if (!service) {
        if (failure) *failure = @"MTLCompiler.framework not available";
        return nil;
    }
    __block NSData *metallib = nil;
    __block NSString *warned = nil, *message = nil;
    __block bool called = false;
    {
        std::lock_guard<std::mutex> guard(lock);
        xcmOnLargeStack(^{
            build(service, nullptr, type, request.data(), request.size(),
                  ^(int32_t status, const void *reply, size_t length, const char *why) {
                      called = true;
                      uint32_t hdr = 0, warn = 0;
                      if (reply && length >= 16) {
                          memcpy(&hdr, (const uint8_t *)reply + 8, 4);
                          memcpy(&warn, (const uint8_t *)reply + 12, 4);
                      }
                      if (status || !reply || length < 16 || (uint64_t)hdr + warn > length) {
                          message = (why && *why) ? @(why)
                                                  : [NSString stringWithFormat:@"MTLCompiler failed (status %d)", status];
                          return;
                      }
                      if (warn) {
                          warned = [[NSString alloc] initWithBytes:(const uint8_t *)reply + hdr
                                                            length:warn
                                                          encoding:NSUTF8StringEncoding];
                      }
                      metallib = [NSData dataWithBytes:(const uint8_t *)reply + hdr + warn
                                                length:length - hdr - warn];
                  });
        });
    }
    if (!called) message = @"MTLCompiler did not respond";
    if (warnings) *warnings = warned;
    if (failure) *failure = message;
    return metallib;
}

/* ======================== Base Object ======================== */

@interface XcmObject : NSObject
@property (nonatomic, copy) NSString *label;
@end
@implementation XcmObject
- (uint64_t)globalTraceObjectID { return 0; }
@end

/* ======================== Buffer ======================== */

static std::atomic<uint64_t> gNoCopyAliased{0};
static std::atomic<uint64_t> gNoCopyFallbackNoDeallocator{0};
static std::atomic<uint64_t> gNoCopyFallbackUnaligned{0};
static std::atomic<uint64_t> gNoCopyFallbackReadOnly{0};
static std::atomic<uint64_t> gNoCopyFallbackRemapErr{0};
static std::atomic<uint64_t> gNoCopyCommitSyncCopies{0};
static std::atomic<uint64_t> gNoCopyCommitSyncBytes{0};
static std::atomic<uint64_t> gNoCopyMaxAliasedLen{0};
static std::atomic<uint64_t> gNoCopyMaxNoDeallocLen{0};
static std::atomic<uint64_t> gNoCopyLastLogNs{0};

static void xcmAtomicMax(std::atomic<uint64_t> &slot, uint64_t val) {
    uint64_t cur = slot.load(std::memory_order_relaxed);
    while (val > cur && !slot.compare_exchange_weak(cur, val, std::memory_order_relaxed)) {}
}

static void xcmMaybeLogNoCopyStats(void) {
    uint64_t now = clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW);
    uint64_t prev = gNoCopyLastLogNs.load(std::memory_order_relaxed);
    if (prev != 0 && now - prev < 15ULL * 1000000000ULL) return;
    if (!gNoCopyLastLogNs.compare_exchange_strong(prev, now, std::memory_order_relaxed)) return;
    os_log_info(OS_LOG_DEFAULT,
                "XnuCpuMetal: NoCopy stats aliased=%llu(max=%llu) noDealloc=%llu(max=%llu) ro=%llu unal=%llu remapErr=%llu commitSyncCopies=%llu commitSyncBytes=%llu",
                (unsigned long long)gNoCopyAliased.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyMaxAliasedLen.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyFallbackNoDeallocator.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyMaxNoDeallocLen.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyFallbackReadOnly.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyFallbackUnaligned.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyFallbackRemapErr.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyCommitSyncCopies.load(std::memory_order_relaxed),
                (unsigned long long)gNoCopyCommitSyncBytes.load(std::memory_order_relaxed));
}

static BOOL xcmRangeIsFullyWritable(void *ptr, size_t len) {
    if (!ptr || len == 0) return NO;
    mach_vm_address_t cur = (mach_vm_address_t)ptr;
    mach_vm_address_t end = cur + (mach_vm_size_t)len;
    if (end < cur) return NO;
    while (cur < end) {
        mach_vm_address_t regAddr = cur;
        mach_vm_size_t regSize = 0;
        vm_region_basic_info_data_64_t info;
        mach_msg_type_number_t count = VM_REGION_BASIC_INFO_COUNT_64;
        mach_port_t obj = MACH_PORT_NULL;
        kern_return_t kr = mach_vm_region(mach_task_self(), &regAddr, &regSize,
                                          VM_REGION_BASIC_INFO_64,
                                          (vm_region_info_t)&info, &count, &obj);
        if (obj != MACH_PORT_NULL) mach_port_deallocate(mach_task_self(), obj);
        if (kr != KERN_SUCCESS || regSize == 0 || regAddr > cur) return NO;
        if ((info.protection & VM_PROT_WRITE) == 0) return NO;
        mach_vm_address_t next = regAddr + regSize;
        if (next <= cur) return NO;
        cur = next;
    }
    return YES;
}

@interface XcmBuffer : XcmObject {
@public
    Xm2vBuffer *_handle;
    id _device;
    MTLResourceOptions _options;
    uint64_t _length;
    MTLPurgeableState _purgeable;
    void (^_fallbackDeallocator)(void *, NSUInteger);
    void *_fallbackPtr;
    NSUInteger _fallbackLen;
    BOOL _fallbackAliased;
    BOOL _fallbackReadOnly;
    BOOL _contentsAccessed;
}
- (instancetype)initWithHandle:(Xm2vBuffer *)h device:(id)dev options:(MTLResourceOptions)opts;
- (void)syncFromFallbackHostMemory;
@end

@implementation XcmBuffer
- (instancetype)initWithHandle:(Xm2vBuffer *)h device:(id)dev options:(MTLResourceOptions)opts {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _options = opts;
        _length = xm2v_buffer_length(h);
        _purgeable = MTLPurgeableStateNonVolatile;
    }
    return self;
}
- (void)dealloc {
    if (_fallbackAliased && _fallbackDeallocator && _fallbackPtr && _fallbackLen) {
        // Detach caller's VA from the GPU blob's physical pages BEFORE releasing _handle
        // (XnuDrm::doUnmap releases the Linux physical pages immediately on XNUDRM_UNMAP).
        // Aliasing is only enabled when `_fallbackDeallocator != nil`, so the Metal NoCopy
        // contract guarantees `_fallbackPtr` remains mapped and exclusively owned by this
        // buffer until `_fallbackDeallocator` is invoked below.
        mach_vm_address_t addr = (mach_vm_address_t)_fallbackPtr;
        if (xcmRangeIsFullyWritable(_fallbackPtr, _fallbackLen)) {
            mach_vm_allocate(mach_task_self(), &addr, (mach_vm_size_t)_fallbackLen,
                             VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE);
        }
        _fallbackAliased = NO;
    }
    if (_handle) { xm2v_buffer_release(_handle); _handle = nullptr; }
    if (_fallbackDeallocator) {
        void (^d)(void *, NSUInteger) = _fallbackDeallocator;
        _fallbackDeallocator = nil;
        d(_fallbackPtr, (NSUInteger)_length);
    }
}
- (void)syncFromFallbackHostMemory {
    // True physical-page alias & read-only buffers never need per-commit host copies.
    if (_fallbackAliased || _fallbackReadOnly || !_fallbackPtr || !_fallbackLen || !_handle || _contentsAccessed) return;
    void *dst = xm2v_buffer_contents(_handle);
    if (dst && dst != _fallbackPtr) {
        memcpy(dst, _fallbackPtr, _fallbackLen);
        gNoCopyCommitSyncCopies.fetch_add(1, std::memory_order_relaxed);
        gNoCopyCommitSyncBytes.fetch_add((uint64_t)_fallbackLen, std::memory_order_relaxed);
        xcmMaybeLogNoCopyStats();
        if (self.storageMode == MTLStorageModeManaged) {
            xm2v_buffer_did_modify_range(_handle, 0, _fallbackLen);
        }
    }
}
- (id<MTLDevice>)device { return _device; }
- (void *)contents {
    if (_fallbackAliased) return _fallbackPtr;
    _contentsAccessed = YES;
    return _handle ? xm2v_buffer_contents(_handle) : nullptr;
}
- (NSUInteger)length { return (NSUInteger)_length; }
- (uint64_t)gpuAddress { return _handle ? xm2v_buffer_gpu_address(_handle) : 0; }
- (void)didModifyRange:(NSRange)range {
    [self syncFromFallbackHostMemory];
    if (_handle) xm2v_buffer_did_modify_range(_handle, range.location, range.length);
}
- (MTLResourceOptions)resourceOptions { return _options; }
- (MTLStorageMode)storageMode { return (MTLStorageMode)((_options & MTLResourceStorageModeMask) >> MTLResourceStorageModeShift); }
- (MTLCPUCacheMode)cpuCacheMode { return (MTLCPUCacheMode)((_options & MTLResourceCPUCacheModeMask) >> MTLResourceCPUCacheModeShift); }
- (MTLHazardTrackingMode)hazardTrackingMode { return (MTLHazardTrackingMode)((_options & MTLResourceHazardTrackingModeMask) >> MTLResourceHazardTrackingModeShift); }
- (NSUInteger)allocatedSize { return (NSUInteger)_length; }
- (MTLPurgeableState)setPurgeableState:(MTLPurgeableState)state {
    MTLPurgeableState prev = _purgeable;
    if (state != MTLPurgeableStateKeepCurrent) _purgeable = state;
    return prev;
}
@end

/* ======================== Texture (with IOSurface bidirectional sync) ======================== */

@interface XcmTexture : XcmObject {
@public
    Xm2vTexture *_handle;
    id _device;
    MTLTextureType _textureType;
    MTLPixelFormat _pixelFormat;
    NSUInteger _width, _height, _depth, _mipLevels, _sampleCount, _arrayLength;
    MTLTextureUsage _usage;
    MTLResourceOptions _options;
    MTLPurgeableState _purgeable;
    IOSurfaceRef _iosurface;
    NSUInteger _iosurfacePlane;
    uint32_t _iosurfaceSeed;
    BOOL _iosurfaceSyncedIn;
    BOOL _iosurfaceIsYCbCrBacking;
    XcmBuffer *_buffer;
    NSUInteger _bufferOffset;
    NSUInteger _bufferBytesPerRow;
}
- (instancetype)initWithHandle:(Xm2vTexture *)h device:(id)dev descriptor:(MTLTextureDescriptor *)desc;
- (void)syncFromIOSurfaceIfNeeded;
- (void)syncToIOSurface;
@end

@implementation XcmTexture
- (instancetype)initWithHandle:(Xm2vTexture *)h device:(id)dev descriptor:(MTLTextureDescriptor *)desc {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _textureType = desc.textureType;
        _pixelFormat = desc.pixelFormat;
        _width = desc.width;
        _height = desc.height;
        _depth = desc.depth;
        _mipLevels = desc.mipmapLevelCount;
        _sampleCount = desc.sampleCount;
        _arrayLength = desc.arrayLength;
        _usage = desc.usage;
        _options = desc.resourceOptions;
        _purgeable = MTLPurgeableStateNonVolatile;
    }
    return self;
}
- (void)dealloc {
    if (_handle) { xm2v_texture_release(_handle); _handle = nullptr; }
    if (_iosurface) { CFRelease(_iosurface); _iosurface = nullptr; }
}
static std::mutex gIOSurfaceWriterLock;
static std::unordered_map<uint32_t, __weak id> gIOSurfaceLastWriter;

static void xcmRecordIOSurfaceWriter(IOSurfaceRef surf, id cmdBuf) {
    if (!surf || !cmdBuf) return;
    uint32_t sid = IOSurfaceGetID(surf);
    if (!sid) return;
    std::lock_guard<std::mutex> lk(gIOSurfaceWriterLock);
    if (gIOSurfaceLastWriter.size() > 256) {
        for (auto it = gIOSurfaceLastWriter.begin(); it != gIOSurfaceLastWriter.end(); ) {
            id w = it->second;
            if (!w) it = gIOSurfaceLastWriter.erase(it);
            else ++it;
        }
    }
    gIOSurfaceLastWriter[sid] = cmdBuf;
}

static void xcmFlushIOSurfaceWriter(IOSurfaceRef surf) {
    if (!surf) return;
    uint32_t sid = IOSurfaceGetID(surf);
    if (!sid) return;
    id writer = nil;
    {
        std::lock_guard<std::mutex> lk(gIOSurfaceWriterLock);
        auto it = gIOSurfaceLastWriter.find(sid);
        if (it != gIOSurfaceLastWriter.end()) writer = it->second;
    }
    if (writer && [writer respondsToSelector:@selector(waitUntilCompleted)] &&
        [(id<MTLCommandBuffer>)writer status] >= MTLCommandBufferStatusCommitted) {
        [(id<MTLCommandBuffer>)writer waitUntilCompleted];
    }
}

static NSUInteger xcmPixelFormatBpp(MTLPixelFormat fmt) {
    switch (fmt) {
        case MTLPixelFormatA8Unorm: case MTLPixelFormatR8Unorm: case MTLPixelFormatR8Snorm:
        case MTLPixelFormatR8Uint: case MTLPixelFormatR8Sint: case MTLPixelFormatStencil8:
            return 1;
        case MTLPixelFormatR16Unorm: case MTLPixelFormatR16Snorm: case MTLPixelFormatR16Uint:
        case MTLPixelFormatR16Sint: case MTLPixelFormatR16Float: case MTLPixelFormatRG8Unorm:
        case MTLPixelFormatRG8Snorm: case MTLPixelFormatRG8Uint: case MTLPixelFormatRG8Sint:
        case MTLPixelFormatDepth16Unorm:
            return 2;
        case MTLPixelFormatRGBA16Unorm: case MTLPixelFormatRGBA16Snorm: case MTLPixelFormatRGBA16Uint:
        case MTLPixelFormatRGBA16Sint: case MTLPixelFormatRGBA16Float:
        case MTLPixelFormatRG32Uint: case MTLPixelFormatRG32Sint: case MTLPixelFormatRG32Float:
        case MTLPixelFormatDepth32Float_Stencil8:
        case MTLPixelFormatBGRA10_XR: case MTLPixelFormatBGRA10_XR_sRGB:
            return 8;
        case MTLPixelFormatRGBA32Uint: case MTLPixelFormatRGBA32Sint: case MTLPixelFormatRGBA32Float:
            return 16;
        default:
            return 4;
    }
}

// Resolve linear base pointer and bytes-per-row for an IOSurface plane (while locked),
// treating '&'-prefixed lossless-compressed formats (&BGA, &b3a, &RhA, &R10, etc.) as linear
// uncompressed rows and computing 64-byte aligned planes for 2-plane b3a8/&b3a when bpr==0.
static bool xcmIOSurfacePlaneLayout(IOSurfaceRef surf, NSUInteger plane, NSUInteger width, NSUInteger height,
                                    MTLPixelFormat fmt, void **outBase, size_t *outBpr) {
    if (!surf || !width || !height) return false;
    void *rootBase = IOSurfaceGetBaseAddress(surf);
    size_t allocSz = IOSurfaceGetAllocSize(surf);
    size_t planeCount = IOSurfaceGetPlaneCount(surf);
    bool usePlane = (planeCount > 0 && plane < planeCount);
    void *base = usePlane ? IOSurfaceGetBaseAddressOfPlane(surf, plane) : rootBase;
    size_t bpr = usePlane ? IOSurfaceGetBytesPerRowOfPlane(surf, plane) : IOSurfaceGetBytesPerRow(surf);

    if (bpr == 0) {
        uint32_t pf = IOSurfaceGetPixelFormat(surf);
        // 2-plane RGB10A8_2P_XR10 ('b3a8' or compressed '&b3a') reporting 0 plane row bytes.
        if ((pf == 0x62336138u || pf == 0x26623361u) && plane < 2 && rootBase) {
            size_t sw = IOSurfaceGetWidth(surf), sh = IOSurfaceGetHeight(surf);
            size_t row0 = (sw * 4 + 63) & ~(size_t)63;
            size_t row1 = (sw + 63) & ~(size_t)63;
            if (sw && sh && row0 * sh + row1 * sh <= allocSz) {
                base = (uint8_t *)rootBase + (plane == 0 ? 0 : row0 * sh);
                bpr = (plane == 0) ? row0 : row1;
            }
        }
        if (bpr == 0) {
            if (!base) base = rootBase;
            bpr = IOSurfaceGetBytesPerRow(surf);
        }
        if (bpr == 0) {
            bpr = ((size_t)width * xcmPixelFormatBpp(fmt) + 63) & ~(size_t)63;
        }
    }
    if (!base || bpr == 0) return false;
    if (rootBase && allocSz > 0) {
        uintptr_t r = (uintptr_t)rootBase, b = (uintptr_t)base;
        if (b < r || (b - r) + bpr * (size_t)height > allocSz) {
            // Clamp to unaligned width*bpp if padded bpr slightly exceeds allocSz on last row.
            size_t minBpr = (size_t)width * xcmPixelFormatBpp(fmt);
            if (b < r || (b - r) + minBpr * (size_t)height > allocSz) return false;
        }
    }
    *outBase = base;
    *outBpr = bpr;
    return true;
}

- (void)syncFromIOSurfaceIfNeeded {
    if (!_iosurface || !_handle) return;
    xcmFlushIOSurfaceWriter(_iosurface);
    @synchronized(self) {
        if (!_iosurface || !_handle) return;
        uint32_t seed = IOSurfaceGetSeed(_iosurface);
        if (_iosurfaceSyncedIn && seed == _iosurfaceSeed) return;
        if (IOSurfaceLock(_iosurface, kIOSurfaceLockReadOnly, nullptr) == kIOReturnSuccess) {
            if (_iosurfaceIsYCbCrBacking && _buffer && _buffer->_handle) {
                void *src = IOSurfaceGetBaseAddress(_iosurface);
                size_t allocSz = IOSurfaceGetAllocSize(_iosurface);
                void *dst = xm2v_buffer_contents(_buffer->_handle);
                size_t copySz = allocSz < (size_t)_buffer->_length ? allocSz : (size_t)_buffer->_length;
                if (src && dst && copySz > 0) memcpy(dst, src, copySz);
            } else {
                void *base = nullptr;
                size_t bpr = 0;
                if (xcmIOSurfacePlaneLayout(_iosurface, _iosurfacePlane, _width, _height, _pixelFormat, &base, &bpr)) {
                    xm2v_texture_replace_region(_handle, 0, 0, 0, _width, _height, 1, 0, 0, base, bpr, 0);
                }
            }
            IOSurfaceUnlock(_iosurface, kIOSurfaceLockReadOnly, nullptr);
            _iosurfaceSeed = seed;
            _iosurfaceSyncedIn = YES;
        }
    }
}
- (void)syncToIOSurface {
    @synchronized(self) {
        if (!_iosurface || !_handle) return;
        if (IOSurfaceLock(_iosurface, 0, nullptr) == kIOReturnSuccess) {
            void *base = nullptr;
            size_t bpr = 0;
            if (xcmIOSurfacePlaneLayout(_iosurface, _iosurfacePlane, _width, _height, _pixelFormat, &base, &bpr)) {
                bool ok = xm2v_texture_get_bytes(_handle, base, bpr, 0, 0, 0, 0, _width, _height, 1, 0, 0);
                if (!ok) {
                    static int sSyncFailLogs = 0;
                    if (sSyncFailLogs < 10) {
                        sSyncFailLogs++;
                        os_log_error(OS_LOG_DEFAULT,
                                     "XnuCpuMetal: syncToIOSurface failed (sid=0x%x surfFmt=0x%x fmt=%lu %lux%lu bpr=%zu): %{public}s",
                                     IOSurfaceGetID(_iosurface), IOSurfaceGetPixelFormat(_iosurface),
                                     (unsigned long)_pixelFormat, (unsigned long)_width, (unsigned long)_height,
                                     bpr, xm2v_last_error() ?: "?");
                    }
                }
                if (xcmTraceOn() || !ok) {
                    const uint8_t *p = (const uint8_t *)base;
                    size_t nz = 0;
                    for (size_t i = 0; i < bpr * _height; i++) if (p[i]) nz++;
                    fprintf(stderr, "XCM syncToIOSurface(%p surf=%p fmt=%lu %lux%lu bpr=%zu) ok=%d nz=%zu/%zu err=%s\n",
                            (__bridge void *)self, _iosurface, (unsigned long)_pixelFormat,
                            (unsigned long)_width, (unsigned long)_height, bpr, ok, nz, bpr * _height,
                            ok ? "ok" : xm2v_last_error());
                }
            }
            IOSurfaceUnlock(_iosurface, 0, nullptr);
            _iosurfaceSeed = IOSurfaceGetSeed(_iosurface);
            _iosurfaceSyncedIn = YES;
        }
    }
}
- (id<MTLDevice>)device { return _device; }
- (MTLTextureType)textureType { return _textureType; }
- (MTLPixelFormat)pixelFormat { return _pixelFormat; }
- (NSUInteger)width { return _width; }
- (NSUInteger)height { return _height; }
- (NSUInteger)depth { return _depth; }
- (NSUInteger)mipmapLevelCount { return _mipLevels; }
- (NSUInteger)sampleCount { return _sampleCount; }
- (NSUInteger)arrayLength { return _arrayLength; }
- (MTLTextureUsage)usage { return _usage; }
- (MTLResourceOptions)resourceOptions { return _options; }
- (MTLStorageMode)storageMode { return (MTLStorageMode)((_options & MTLResourceStorageModeMask) >> MTLResourceStorageModeShift); }
- (MTLCPUCacheMode)cpuCacheMode { return (MTLCPUCacheMode)((_options & MTLResourceCPUCacheModeMask) >> MTLResourceCPUCacheModeShift); }
- (MTLHazardTrackingMode)hazardTrackingMode { return (MTLHazardTrackingMode)((_options & MTLResourceHazardTrackingModeMask) >> MTLResourceHazardTrackingModeShift); }
- (BOOL)isFramebufferOnly { return NO; }
- (BOOL)allowGPUOptimizedContents { return YES; }
- (BOOL)isDrawable { return NO; }
- (NSUInteger)protectionOptions { return 0; }
- (NSUInteger)allocatedSize {
    NSUInteger bpp = xcmPixelFormatBpp(_pixelFormat);
    NSUInteger sz = MAX(1ul, _width) * MAX(1ul, _height) * MAX(1ul, _depth) * MAX(1ul, _arrayLength) * MAX(1ul, _sampleCount) * bpp;
    return (sz + 255ul) & ~255ul;
}
- (id<MTLTexture>)parentTexture { return nil; }
- (id<MTLBuffer>)buffer { return (id<MTLBuffer>)_buffer; }
- (NSUInteger)bufferOffset { return _bufferOffset; }
- (NSUInteger)bufferBytesPerRow { return _bufferBytesPerRow; }
- (IOSurfaceRef)iosurface { return _iosurface; }
- (NSUInteger)iosurfacePlane { return _iosurfacePlane; }
- (MTLPurgeableState)setPurgeableState:(MTLPurgeableState)state {
    MTLPurgeableState prev = _purgeable;
    if (state != MTLPurgeableStateKeepCurrent) _purgeable = state;
    return prev;
}
- (void)replaceRegion:(MTLRegion)region mipmapLevel:(NSUInteger)level slice:(NSUInteger)slice
            withBytes:(const void *)pixelBytes bytesPerRow:(NSUInteger)bytesPerRow bytesPerImage:(NSUInteger)bytesPerImage {
    if (_handle) {
        // The write-back below is the whole texture: what the surface holds now goes in first, or the rest of what was
        // drawn into the surface since the last sync is lost to a write of one region.
        if (_iosurface && level == 0 && slice == 0) [self syncFromIOSurfaceIfNeeded];
        xm2v_texture_replace_region(_handle,
            region.origin.x, region.origin.y, region.origin.z,
            region.size.width, region.size.height, region.size.depth,
            level, slice, pixelBytes, bytesPerRow, bytesPerImage);
        if (_iosurface && level == 0 && slice == 0) [self syncToIOSurface];
    }
}
- (void)replaceRegion:(MTLRegion)region mipmapLevel:(NSUInteger)level
            withBytes:(const void *)pixelBytes bytesPerRow:(NSUInteger)bytesPerRow {
    [self replaceRegion:region mipmapLevel:level slice:0 withBytes:pixelBytes bytesPerRow:bytesPerRow bytesPerImage:0];
}
- (void)getBytes:(void *)pixelBytes bytesPerRow:(NSUInteger)bytesPerRow bytesPerImage:(NSUInteger)bytesPerImage
      fromRegion:(MTLRegion)region mipmapLevel:(NSUInteger)level slice:(NSUInteger)slice {
    if (_handle) {
        if (_iosurface && level == 0 && slice == 0) [self syncFromIOSurfaceIfNeeded];
        xm2v_texture_get_bytes(_handle, pixelBytes, bytesPerRow, bytesPerImage,
            region.origin.x, region.origin.y, region.origin.z,
            region.size.width, region.size.height, region.size.depth,
            level, slice);
    }
}
- (void)getBytes:(void *)pixelBytes bytesPerRow:(NSUInteger)bytesPerRow
      fromRegion:(MTLRegion)region mipmapLevel:(NSUInteger)level {
    [self getBytes:pixelBytes bytesPerRow:bytesPerRow bytesPerImage:0 fromRegion:region mipmapLevel:level slice:0];
}
- (id<MTLTexture>)newTextureViewWithPixelFormat:(MTLPixelFormat)pixelFormat
                                    textureType:(MTLTextureType)textureType
                                         levels:(NSRange)levelRange
                                         slices:(NSRange)sliceRange {
    if (!_handle) return nil;
    Xm2vTexture *vh = xm2v_texture_new_view(_handle, (uint64_t)pixelFormat, (uint64_t)textureType,
                                            levelRange.location, levelRange.length,
                                            sliceRange.location, sliceRange.length);
    if (!vh) return nil;
    MTLTextureDescriptor *desc = [[MTLTextureDescriptor alloc] init];
    desc.textureType = textureType;
    desc.pixelFormat = pixelFormat;
    desc.width = MAX(1ul, _width >> levelRange.location);
    desc.height = MAX(1ul, _height >> levelRange.location);
    desc.depth = MAX(1ul, _depth >> levelRange.location);
    desc.mipmapLevelCount = levelRange.length;
    desc.sampleCount = _sampleCount;
    // arrayLength counts cubes for a cube type, and a cube is six of the slices
    bool cubes = textureType == MTLTextureTypeCube || textureType == MTLTextureTypeCubeArray;
    desc.arrayLength = cubes ? MAX(1ul, sliceRange.length / 6) : sliceRange.length;
    desc.usage = _usage;
    desc.resourceOptions = _options;
    XcmTexture *view = [[XcmTexture alloc] initWithHandle:vh device:_device descriptor:desc];
    if (view && _iosurface && levelRange.location == 0 && sliceRange.location == 0) {
        CFRetain(_iosurface);
        view->_iosurface = _iosurface;
        view->_iosurfacePlane = _iosurfacePlane;
        view->_iosurfaceSeed = _iosurfaceSeed;
        view->_iosurfaceSyncedIn = _iosurfaceSyncedIn;
    }
    if (view && _buffer && levelRange.location == 0 && sliceRange.location == 0) {
        view->_buffer = _buffer;
        view->_bufferOffset = _bufferOffset;
        view->_bufferBytesPerRow = _bufferBytesPerRow;
    }
    return (id<MTLTexture>)view;
}
- (id<MTLTexture>)newTextureViewWithPixelFormat:(MTLPixelFormat)pixelFormat {
    bool cubes = _textureType == MTLTextureTypeCube || _textureType == MTLTextureTypeCubeArray;   // six slices each
    return [self newTextureViewWithPixelFormat:pixelFormat textureType:_textureType
                                        levels:NSMakeRange(0, _mipLevels)
                                        slices:NSMakeRange(0, cubes ? _arrayLength * 6 : _arrayLength)];
}
- (id<MTLTexture>)newTextureViewWithPixelFormat:(MTLPixelFormat)pixelFormat
                                    textureType:(MTLTextureType)textureType
                                         levels:(NSRange)levelRange
                                         slices:(NSRange)sliceRange
                                        swizzle:(MTLTextureSwizzleChannels)swizzle {
    (void)swizzle;
    return [self newTextureViewWithPixelFormat:pixelFormat textureType:textureType levels:levelRange slices:sliceRange];
}
- (id<MTLHeap>)heap { return nil; }
- (NSUInteger)heapOffset { return 0; }
- (BOOL)isAliasable { return NO; }
- (MTLTextureSwizzleChannels)swizzle {
    return MTLTextureSwizzleChannelsMake(MTLTextureSwizzleRed, MTLTextureSwizzleGreen, MTLTextureSwizzleBlue, MTLTextureSwizzleAlpha);
}
- (MTLResourceID)gpuResourceID {
    MTLResourceID rid = { _handle ? xm2v_texture_gpu_resource_id(_handle) : 0 };
    return rid;
}
@end

static void xcmFillTextureDesc(MTLTextureDescriptor *desc, Xm2vTextureDesc &td) {
    td = {};
    td.texture_type = (uint64_t)desc.textureType;
    td.pixel_format = (uint64_t)desc.pixelFormat;
    td.width = desc.width;
    td.height = desc.height;
    td.depth = desc.depth;
    td.mipmap_level_count = desc.mipmapLevelCount;
    td.sample_count = desc.sampleCount;
    td.array_length = desc.arrayLength;
    td.resource_options = (uint64_t)desc.resourceOptions;
    td.usage = (uint64_t)desc.usage;
}

@implementation XcmBuffer (LinearTextures)
- (id<MTLTexture>)newTextureWithDescriptor:(MTLTextureDescriptor *)desc offset:(NSUInteger)offset bytesPerRow:(NSUInteger)bytesPerRow {
    if (!_handle || !desc) return nil;
    [self syncFromFallbackHostMemory];
    Xm2vTextureDesc td;
    xcmFillTextureDesc(desc, td);
    Xm2vTexture *t = xm2v_buffer_new_texture(_handle, &td, offset, bytesPerRow);
    if (!t) {
        const char *err = xm2v_last_error();
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: buffer newTexture failed (fmt=%lu %lux%lu off=%lu bpr=%lu len=%llu): %{public}s",
                     (unsigned long)desc.pixelFormat, (unsigned long)desc.width, (unsigned long)desc.height,
                     (unsigned long)offset, (unsigned long)bytesPerRow, (unsigned long long)_length, err ?: "?");
        return nil;
    }
    XcmTexture *tex = [[XcmTexture alloc] initWithHandle:t device:_device descriptor:desc];
    if (tex) {
        tex->_buffer = self;
        tex->_bufferOffset = offset;
        tex->_bufferBytesPerRow = bytesPerRow;
    }
    return (id<MTLTexture>)tex;
}
- (id<MTLTexture>)newLinearTextureWithDescriptor:(MTLTextureDescriptor *)desc offset:(NSUInteger)offset bytesPerRow:(NSUInteger)bytesPerRow bytesPerImage:(NSUInteger)bytesPerImage {
    if (!_handle || !desc) return nil;
    [self syncFromFallbackHostMemory];
    Xm2vTextureDesc td;
    xcmFillTextureDesc(desc, td);
    Xm2vTexture *t = xm2v_buffer_new_linear_texture(_handle, &td, offset, bytesPerRow, bytesPerImage);
    if (!t) {
        const char *err = xm2v_last_error();
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: buffer newLinearTexture failed (fmt=%lu %lux%lu off=%lu bpr=%lu bpi=%lu len=%llu): %{public}s",
                     (unsigned long)desc.pixelFormat, (unsigned long)desc.width, (unsigned long)desc.height,
                     (unsigned long)offset, (unsigned long)bytesPerRow, (unsigned long)bytesPerImage,
                     (unsigned long long)_length, err ?: "?");
        return nil;
    }
    XcmTexture *tex = [[XcmTexture alloc] initWithHandle:t device:_device descriptor:desc];
    if (tex) {
        tex->_buffer = self;
        tex->_bufferOffset = offset;
        tex->_bufferBytesPerRow = bytesPerRow;
    }
    return (id<MTLTexture>)tex;
}
@end

/* ======================== Function & Library ======================== */

@class XcmLibrary;

@interface XcmArgumentEncoder : XcmObject {
@public
    Xm2vArgumentEncoder *_handle;
    id _device;
    id _argumentBuffer;
}
- (instancetype)initWithHandle:(Xm2vArgumentEncoder *)h device:(id)dev;
@end

@interface XcmFunction : XcmObject {
@public
    Xm2vFunction *_handle;
    id _device;
    XcmLibrary *_library;
    NSString *_name;
    NSString *_sourceName;
    MTLFunctionType _functionType;
    MTLFunctionConstantValues *_constantValues;
}
- (instancetype)initWithHandle:(Xm2vFunction *)h device:(id)dev library:(XcmLibrary *)lib name:(NSString *)name type:(MTLFunctionType)type;
@end

@implementation XcmFunction
- (instancetype)initWithHandle:(Xm2vFunction *)h device:(id)dev library:(XcmLibrary *)lib name:(NSString *)name type:(MTLFunctionType)type {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _library = lib;
        _name = [name copy];
        _sourceName = [name copy];
        _functionType = h ? (MTLFunctionType)xm2v_function_type(h) : type;
    }
    return self;
}
- (void)dealloc {
    if (_handle) { xm2v_function_release(_handle); _handle = nullptr; }
}
- (id<MTLDevice>)device { return _device; }
- (NSString *)name { return _name; }
- (MTLFunctionType)functionType { return _functionType; }
- (NSArray *)vertexAttributes { return @[]; }
- (NSArray *)stageInputAttributes { return @[]; }
- (NSDictionary *)functionConstantsDictionary { return @{}; }
- (id<MTLArgumentEncoder>)newArgumentEncoderWithBufferIndex:(NSUInteger)bufferIndex {
    if (!_handle) return nil;
    Xm2vArgumentEncoder *enc = xm2v_function_new_argument_encoder(_handle, (uint32_t)bufferIndex);
    if (!enc) {
        if (xcmTraceOn()) fprintf(stderr, "XCM newArgumentEncoderWithBufferIndex:%lu (%s) FAILED: %s\n",
                                  (unsigned long)bufferIndex, _name.UTF8String ?: "?", xm2v_last_error());
        return nil;
    }
    return (id<MTLArgumentEncoder>)[[XcmArgumentEncoder alloc] initWithHandle:enc device:_device];
}
- (id<MTLArgumentEncoder>)newArgumentEncoderWithBufferIndex:(NSUInteger)bufferIndex
                                                 reflection:(MTLAutoreleasedArgument *)reflection {
    if (reflection) *reflection = nil;
    return [self newArgumentEncoderWithBufferIndex:bufferIndex];
}
@end

@interface XcmLibrary : XcmObject {
@public
    Xm2vLibrary *_handle;
    id _device;
    NSData *_metallibData;
}
- (instancetype)initWithHandle:(Xm2vLibrary *)h device:(id)dev data:(NSData *)data;
@end

static bool xcmForEachMetallibEntry(NSData *data, const std::function<bool(const std::string &, int)> &visit) {
    if (!data || data.length < 88) return false;
    const uint8_t *file = (const uint8_t *)data.bytes;
    size_t length = data.length;
    if (memcmp(file, "MTLB", 4) != 0) return false;
    uint64_t listAt = 0;
    memcpy(&listAt, file + 24, 8);
    if (listAt > length - 4) return false;
    uint32_t count = 0;
    memcpy(&count, file + listAt, 4);
    size_t at = listAt + 4;
    for (uint32_t i = 0; i < count; i++) {
        uint32_t size = 0;
        if (at > length - 4) return false;
        memcpy(&size, file + at, 4);
        if (size < 8 || size > length - at) return false;
        size_t end = at + size;
        std::string entry;
        int type = -1;
        for (size_t t = at + 4; t + 4 <= end && memcmp(file + t, "ENDT", 4) != 0;) {
            if (t + 6 > end) return false;
            uint16_t n = 0;
            memcpy(&n, file + t + 4, 2);
            const uint8_t *v = file + t + 6;
            if (n > end - (t + 6)) return false;
            if (!memcmp(file + t, "NAME", 4)) entry.assign((const char *)v, strnlen((const char *)v, n));
            else if (!memcmp(file + t, "TYPE", 4) && n >= 1) type = v[0];
            t += 6 + n;
        }
        if (!visit(entry, type)) return true;
        at = end;
    }
    return true;
}

static NSData *xcmExtractFunctionBitcode(XcmFunction *fn) {
    if (!fn || !fn->_library || !fn->_library->_metallibData) return nil;
    NSData *data = fn->_library->_metallibData;
    const uint8_t *file = (const uint8_t *)data.bytes;
    size_t length = data.length;
    const char *wanted = (fn->_sourceName ?: fn->_name).UTF8String;
    if (!file || !wanted || length < 88 || memcmp(file, "MTLB", 4) != 0) return nil;
    uint64_t listAt = 0, bitcodeAt = 0, bitcodeSize = 0;
    memcpy(&listAt, file + 24, 8);
    memcpy(&bitcodeAt, file + 72, 8);
    memcpy(&bitcodeSize, file + 80, 8);
    if (listAt > length - 4 || bitcodeAt > length || bitcodeSize > length - bitcodeAt) return nil;
    uint32_t count = 0;
    memcpy(&count, file + listAt, 4);
    size_t at = listAt + 4;
    for (uint32_t i = 0; i < count; i++) {
        uint32_t size = 0;
        if (at > length - 4) return nil;
        memcpy(&size, file + at, 4);
        if (size < 8 || size > length - at) return nil;
        size_t end = at + size;
        std::string name;
        uint64_t bytes = 0, offset = 0;
        bool sized = false, placed = false;
        for (size_t t = at + 4; t + 4 <= end && memcmp(file + t, "ENDT", 4) != 0;) {
            if (t + 6 > end) return nil;
            uint16_t n = 0;
            memcpy(&n, file + t + 4, 2);
            const uint8_t *v = file + t + 6;
            if (n > end - (t + 6)) return nil;
            if (!memcmp(file + t, "NAME", 4)) name.assign((const char *)v, strnlen((const char *)v, n));
            else if (!memcmp(file + t, "MDSZ", 4) && n >= 8) { memcpy(&bytes, v, 8); sized = true; }
            else if (!memcmp(file + t, "OFFT", 4) && n >= 24) { memcpy(&offset, v + 16, 8); placed = true; }
            t += 6 + n;
        }
        if (name == wanted) {
            if (!sized || !placed || offset > bitcodeSize || bytes > bitcodeSize - offset) return nil;
            return [NSData dataWithBytes:file + bitcodeAt + offset length:(NSUInteger)bytes];
        }
        at = end;
    }
    return nil;
}

static XcmFunction *xcmLookupCIKernelFunction(XcmLibrary *lib, NSString *name) {
    if (!lib || !lib->_metallibData || !name || lib->_metallibData.length < 88) return nil;
    const uint8_t *file = (const uint8_t *)lib->_metallibData.bytes;
    if (file[10] != 1) return nil; // file_type 1 = CoreImage / dynamic kernel library
    const char *wanted = name.UTF8String;
    if (!wanted) return nil;
    XcmFunction *found = nil;
    xcmForEachMetallibEntry(lib->_metallibData, [&](const std::string &entry, int type) {
        if (entry != wanted) return true;
        if (type == 5) {
            found = [[XcmFunction alloc] initWithHandle:nullptr device:lib->_device library:lib name:name type:(MTLFunctionType)4];
        }
        return false;
    });
    return found;
}

@implementation XcmLibrary
- (instancetype)initWithHandle:(Xm2vLibrary *)h device:(id)dev data:(NSData *)data {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _metallibData = data;
    }
    return self;
}
- (void)dealloc {
    if (_handle) { xm2v_library_release(_handle); _handle = nullptr; }
}
- (id<MTLDevice>)device { return _device; }
- (MTLLibraryType)type { return MTLLibraryTypeExecutable; }
- (NSString *)installName { return nil; }
- (NSArray<NSString *> *)functionNames {
    NSMutableArray<NSString *> *arr = [NSMutableArray array];
    if (!_handle) return arr;
    size_t count = xm2v_library_function_count(_handle);
    char buf[512];
    for (size_t i = 0; i < count; i++) {
        size_t len = xm2v_library_function_name_at(_handle, i, buf, sizeof(buf));
        if (len > 0) [arr addObject:[NSString stringWithUTF8String:buf]];
    }
    return arr;
}
- (NSArray<NSString *> *)externFunctionNames {
    NSMutableArray<NSString *> *arr = [NSMutableArray array];
    if (_handle) {
        size_t count = xm2v_library_extern_function_count(_handle);
        char buf[512];
        for (size_t i = 0; i < count; i++) {
            size_t len = xm2v_library_extern_function_name_at(_handle, i, buf, sizeof(buf));
            if (len > 0) [arr addObject:[NSString stringWithUTF8String:buf]];
        }
    }
    if (arr.count == 0 && _metallibData) {
        xcmForEachMetallibEntry(_metallibData, [&](const std::string &entry, int type) {
            if (type == 5 && !entry.empty()) [arr addObject:@(entry.c_str())];
            return true;
        });
    }
    return arr;
}
- (id<MTLFunction>)newFunctionWithName:(NSString *)functionName {
    if (!functionName) return nil;
    if (XcmFunction *ciFn = xcmLookupCIKernelFunction(self, functionName))
        return (id<MTLFunction>)ciFn;
    if (!_handle) return nil;
    Xm2vFunction *fn = xm2v_library_new_function(_handle, functionName.UTF8String);
    if (!fn) fn = xm2v_library_new_extern_function(_handle, functionName.UTF8String);
    if (!fn) return nil;
    return (id<MTLFunction>)[[XcmFunction alloc] initWithHandle:fn device:_device library:self name:functionName type:MTLFunctionTypeKernel];
}
static void xcmExtractConstants(MTLFunctionConstantValues *constantValues,
                                std::vector<Xm2vConstant> &constants,
                                std::deque<std::string> &nameStorage,
                                std::deque<std::vector<uint8_t>> &byteStorage) {
    if (!constantValues) return;
    MTLFunctionConstantValues *snap = [constantValues copy];
    NSArray *indexed = [snap respondsToSelector:@selector(newIndexedConstantArray)] ? [snap newIndexedConstantArray] : nil;
    NSArray *named = [snap respondsToSelector:@selector(newNamedConstantArray)] ? [snap newNamedConstantArray] : nil;
    for (int grp = 0; grp < 2; grp++) {
        for (id<XcmPrivateConstantValue> v in (grp ? named : indexed)) {
            uint64_t dt = v.dataType;
            uint64_t sz = xcmConstantBytes(dt);
            void *ptr = v.data;
            if (!sz || !ptr) continue;
            byteStorage.emplace_back((const uint8_t *)ptr, (const uint8_t *)ptr + (size_t)sz);
            Xm2vConstant c = {};
            c.data_type = dt;
            if (grp == 0) {
                c.index = (uint32_t)v.index;
                c.name = nullptr;
            } else {
                nameStorage.emplace_back(v.name.UTF8String ?: "");
                c.name = nameStorage.back().c_str();
            }
            c.bytes = byteStorage.back().data();
            c.length = (size_t)sz;
            constants.push_back(c);
        }
    }
}

- (id<MTLFunction>)newFunctionWithName:(NSString *)name
                        constantValues:(MTLFunctionConstantValues *)constantValues
                                 error:(NSError **)error {
    if (!name) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFunctionNotFound, "Nil function name");
        return nil;
    }
    if (XcmFunction *ciFn = xcmLookupCIKernelFunction(self, name)) {
        if (error) *error = nil;
        return (id<MTLFunction>)ciFn;
    }
    if (!_handle) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Null library");
        return nil;
    }
    std::vector<Xm2vConstant> constants;
    std::deque<std::string> nameStorage;
    std::deque<std::vector<uint8_t>> byteStorage;
    xcmExtractConstants(constantValues, constants, nameStorage, byteStorage);
    Xm2vFunction *fn = constants.empty()
        ? xm2v_library_new_function(_handle, name.UTF8String)
        : xm2v_library_new_function_specialized(_handle, name.UTF8String, constants.data(), (uint32_t)constants.size());
    if (!fn) fn = xm2v_library_new_extern_function(_handle, name.UTF8String);
    if (!fn) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFunctionNotFound, "Function not found");
        return nil;
    }
    if (error) *error = nil;
    XcmFunction *out = [[XcmFunction alloc] initWithHandle:fn device:_device library:self name:name type:MTLFunctionTypeKernel];
    if (constantValues) out->_constantValues = [constantValues copy];
    return (id<MTLFunction>)out;
}
- (void)newFunctionWithName:(NSString *)name
             constantValues:(MTLFunctionConstantValues *)constantValues
          completionHandler:(void (^)(id<MTLFunction>, NSError *))completionHandler {
    NSError *err = nil;
    id<MTLFunction> fn = [self newFunctionWithName:name constantValues:constantValues error:&err];
    if (completionHandler) completionHandler(fn, err);
}
- (id<MTLFunction>)newFunctionWithDescriptor:(MTLFunctionDescriptor *)d error:(NSError **)error {
    if (!d || !d.name) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFunctionNotFound, "Missing function name");
        return nil;
    }
    if (XcmFunction *ciFn = xcmLookupCIKernelFunction(self, d.name)) {
        if (d.specializedName) ciFn->_name = [d.specializedName copy];
        if (error) *error = nil;
        return (id<MTLFunction>)ciFn;
    }
    if (!_handle) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Null library");
        return nil;
    }
    std::vector<Xm2vConstant> constants;
    std::deque<std::string> nameStorage;
    std::deque<std::vector<uint8_t>> byteStorage;
    xcmExtractConstants(d.constantValues, constants, nameStorage, byteStorage);
    Xm2vFunction *fn = xm2v_library_new_function_with_descriptor(
        _handle,
        d.name.UTF8String,
        d.specializedName ? d.specializedName.UTF8String : nullptr,
        constants.empty() ? nullptr : constants.data(),
        (uint32_t)constants.size(),
        (uint64_t)d.options);
    if (!fn) fn = xm2v_library_new_extern_function(_handle, d.name.UTF8String);
    if (!fn) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFunctionNotFound, "Function not found");
        return nil;
    }
    if (error) *error = nil;
    XcmFunction *out = [[XcmFunction alloc] initWithHandle:fn device:_device library:self name:d.name type:MTLFunctionTypeKernel];
    if (d.specializedName) out->_name = [d.specializedName copy];
    if (d.constantValues) out->_constantValues = [d.constantValues copy];
    return (id<MTLFunction>)out;
}
- (void)newFunctionWithDescriptor:(MTLFunctionDescriptor *)d
                completionHandler:(void (^)(id<MTLFunction>, NSError *))completionHandler {
    NSError *err = nil;
    id<MTLFunction> fn = [self newFunctionWithDescriptor:d error:&err];
    if (completionHandler) completionHandler(fn, err);
}
@end

/* ======================== Pipelines & States ======================== */

@interface XcmComputePipeline : XcmObject {
@public
    Xm2vComputePipeline *_handle;
    id _device;
}
- (instancetype)initWithHandle:(Xm2vComputePipeline *)h device:(id)dev;
@end

@implementation XcmComputePipeline
- (instancetype)initWithHandle:(Xm2vComputePipeline *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_compute_pipeline_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (NSUInteger)maxTotalThreadsPerThreadgroup { return _handle ? (NSUInteger)xm2v_compute_pipeline_max_threads(_handle) : 64; }
- (NSUInteger)threadExecutionWidth { return _handle ? (NSUInteger)xm2v_compute_pipeline_thread_width(_handle) : 4; }
- (NSUInteger)staticThreadgroupMemoryLength { return _handle ? (NSUInteger)xm2v_compute_pipeline_static_tg_mem(_handle) : 0; }
- (BOOL)supportIndirectCommandBuffers { return NO; }
@end

@interface XcmRenderPipeline : XcmObject {
@public
    Xm2vRenderPipeline *_handle;
    id _device;
    NSString *_vName;
    NSString *_fName;
    BOOL _blendEnabled;
    uint64_t _writeMask;
}
- (instancetype)initWithHandle:(Xm2vRenderPipeline *)h device:(id)dev;
@end

@implementation XcmRenderPipeline
- (instancetype)initWithHandle:(Xm2vRenderPipeline *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_render_pipeline_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (BOOL)supportIndirectCommandBuffers { return NO; }
@end

@interface XcmSampler : XcmObject {
@public
    Xm2vSampler *_handle;
    id _device;
}
- (instancetype)initWithHandle:(Xm2vSampler *)h device:(id)dev;
@end
@implementation XcmSampler
- (instancetype)initWithHandle:(Xm2vSampler *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_sampler_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (MTLResourceID)gpuResourceID {
    MTLResourceID rid = { _handle ? xm2v_sampler_gpu_resource_id(_handle) : 0 };
    return rid;
}
@end

@implementation XcmArgumentEncoder
- (instancetype)initWithHandle:(Xm2vArgumentEncoder *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc {
    if (_handle) { xm2v_argument_encoder_release(_handle); _handle = nullptr; }
}
- (id<MTLDevice>)device { return _device; }
- (NSUInteger)encodedLength { return _handle ? (NSUInteger)xm2v_argument_encoder_encoded_length(_handle) : 0; }
- (NSUInteger)alignment { return _handle ? (NSUInteger)xm2v_argument_encoder_alignment(_handle) : 1; }
- (void)setArgumentBuffer:(id<MTLBuffer>)argumentBuffer offset:(NSUInteger)offset {
    if (!_handle) return;
    _argumentBuffer = argumentBuffer;
    Xm2vBuffer *bh = (argumentBuffer && [argumentBuffer isKindOfClass:[XcmBuffer class]])
        ? ((XcmBuffer *)argumentBuffer)->_handle : nullptr;
    if (!xm2v_argument_encoder_set_argument_buffer(_handle, bh, offset) && xcmTraceOn())
        fprintf(stderr, "XCM argEnc setArgumentBuffer:offset:%lu FAILED: %s\n", (unsigned long)offset, xm2v_last_error());
}
- (void)setArgumentBuffer:(id<MTLBuffer>)argumentBuffer startOffset:(NSUInteger)startOffset arrayElement:(NSUInteger)arrayElement {
    if (!_handle) return;
    _argumentBuffer = argumentBuffer;
    Xm2vBuffer *bh = (argumentBuffer && [argumentBuffer isKindOfClass:[XcmBuffer class]])
        ? ((XcmBuffer *)argumentBuffer)->_handle : nullptr;
    if (!xm2v_argument_encoder_set_argument_buffer_array_element(_handle, bh, startOffset, arrayElement) && xcmTraceOn())
        fprintf(stderr, "XCM argEnc setArgumentBuffer:startOffset:%lu:arrayElement:%lu FAILED: %s\n",
                (unsigned long)startOffset, (unsigned long)arrayElement, xm2v_last_error());
}
- (void)setBuffer:(id<MTLBuffer>)buffer offset:(NSUInteger)offset atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vBuffer *bh = (buffer && [buffer isKindOfClass:[XcmBuffer class]]) ? ((XcmBuffer *)buffer)->_handle : nullptr;
    xm2v_argument_encoder_set_buffer(_handle, bh, offset, (uint32_t)index);
}
- (void)setBuffers:(const id<MTLBuffer> *)buffers offsets:(const NSUInteger *)offsets withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setBuffer:buffers[i] offset:offsets[i] atIndex:range.location + i];
}
- (void)setTexture:(id<MTLTexture>)texture atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vTexture *th = nullptr;
    if (texture && [texture isKindOfClass:[XcmTexture class]]) {
        XcmTexture *xt = (XcmTexture *)texture;
        [xt syncFromIOSurfaceIfNeeded];
        th = xt->_handle;
    }
    xm2v_argument_encoder_set_texture(_handle, th, (uint32_t)index);
}
- (void)setTextures:(const id<MTLTexture> *)textures withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setTexture:textures[i] atIndex:range.location + i];
}
- (void)setSamplerState:(id<MTLSamplerState>)sampler atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = (sampler && [sampler isKindOfClass:[XcmSampler class]]) ? ((XcmSampler *)sampler)->_handle : nullptr;
    xm2v_argument_encoder_set_sampler(_handle, sh, (uint32_t)index);
}
- (void)setSamplerStates:(const id<MTLSamplerState> *)samplers withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setSamplerState:samplers[i] atIndex:range.location + i];
}
- (void)setComputePipelineState:(id<MTLComputePipelineState>)pipeline atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vComputePipeline *ph = (pipeline && [pipeline isKindOfClass:[XcmComputePipeline class]]) ? ((XcmComputePipeline *)pipeline)->_handle : nullptr;
    xm2v_argument_encoder_set_compute_pipeline(_handle, ph, (uint32_t)index);
}
- (void)setComputePipelineStates:(const id<MTLComputePipelineState> *)pipelines withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setComputePipelineState:pipelines[i] atIndex:range.location + i];
}
- (void)setRenderPipelineState:(id<MTLRenderPipelineState>)pipeline atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vRenderPipeline *ph = (pipeline && [pipeline isKindOfClass:[XcmRenderPipeline class]]) ? ((XcmRenderPipeline *)pipeline)->_handle : nullptr;
    xm2v_argument_encoder_set_render_pipeline(_handle, ph, (uint32_t)index);
}
- (void)setRenderPipelineStates:(const id<MTLRenderPipelineState> *)pipelines withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setRenderPipelineState:pipelines[i] atIndex:range.location + i];
}
- (void *)constantDataAtIndex:(NSUInteger)index {
    return _handle ? xm2v_argument_encoder_constant_data(_handle, (uint32_t)index) : nullptr;
}
- (id<MTLArgumentEncoder>)newArgumentEncoderForBufferAtIndex:(NSUInteger)index {
    if (!_handle) return nil;
    Xm2vArgumentEncoder *nested = xm2v_argument_encoder_new_nested(_handle, (uint32_t)index);
    return nested ? (id<MTLArgumentEncoder>)[[XcmArgumentEncoder alloc] initWithHandle:nested device:_device] : nil;
}
@end

@interface XcmDepthStencil : XcmObject {
@public
    Xm2vDepthStencil *_handle;
    id _device;
}
- (instancetype)initWithHandle:(Xm2vDepthStencil *)h device:(id)dev;
@end
@implementation XcmDepthStencil
- (instancetype)initWithHandle:(Xm2vDepthStencil *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_depth_stencil_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
@end

@interface XcmEvent : XcmObject {
@public
    Xm2vEvent *_handle;
    id _device;
}
- (instancetype)initWithHandle:(Xm2vEvent *)h device:(id)dev;
@end
@implementation XcmEvent
- (instancetype)initWithHandle:(Xm2vEvent *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_event_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
@end

@interface XcmSharedEventHandle : NSObject {
@public
    Xm2vSharedEventHandle *_handle;
    NSString *_label;
}
- (instancetype)initWithHandle:(Xm2vSharedEventHandle *)h label:(NSString *)label;
@end

@implementation XcmSharedEventHandle
- (instancetype)initWithHandle:(Xm2vSharedEventHandle *)h label:(NSString *)label {
    if ((self = [super init])) { _handle = h; _label = [label copy]; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_shared_event_handle_release(_handle); _handle = nullptr; } }
- (NSString *)label { return _label; }
@end

@interface XcmSharedEvent : XcmObject {
@public
    Xm2vSharedEvent *_handle;
}
- (instancetype)initWithHandle:(Xm2vSharedEvent *)h;
@end

static void xcmSharedEventNotified(void *context, uint64_t value) {
    NSArray *job = (__bridge_transfer NSArray *)context;
    id<MTLSharedEvent> event = job[0];
    dispatch_queue_t queue = job[1];
    MTLSharedEventNotificationBlock block = job[2];
    dispatch_async(queue, ^{ block(event, value); });
}

@implementation XcmSharedEvent
- (instancetype)initWithHandle:(Xm2vSharedEvent *)h {
    if ((self = [super init])) { _handle = h; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_shared_event_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return nil; }
- (uint64_t)signaledValue { return _handle ? xm2v_shared_event_signaled_value(_handle) : 0; }
- (void)setSignaledValue:(uint64_t)value {
    if (_handle) xm2v_shared_event_set_signaled_value(_handle, value);
}
- (BOOL)waitUntilSignaledValue:(uint64_t)value timeoutMS:(uint64_t)milliseconds {
    return _handle ? (xm2v_shared_event_wait_until(_handle, value, milliseconds) ? YES : NO) : NO;
}
- (void)notifyListener:(MTLSharedEventListener *)listener atValue:(uint64_t)value block:(MTLSharedEventNotificationBlock)block {
    if (!_handle || !block) return;
    dispatch_queue_t queue = listener.dispatchQueue ?: dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0);
    NSArray *job = @[self, queue, [block copy]];
    void *ctx = (__bridge_retained void *)job;
    if (!xm2v_shared_event_notify(_handle, value, xcmSharedEventNotified, ctx)) {
        (void)(__bridge_transfer NSArray *)ctx;
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: shared event notifyListener failed: %{public}s", xm2v_last_error() ?: "?");
    }
}
- (MTLSharedEventHandle *)newSharedEventHandle {
    if (!_handle) return nil;
    Xm2vSharedEventHandle *h = xm2v_shared_event_new_handle(_handle);
    return h ? (MTLSharedEventHandle *)[[XcmSharedEventHandle alloc] initWithHandle:h label:self.label] : nil;
}
@end

@interface XcmFence : XcmObject {
@public
    Xm2vFence *_handle;
    id _device;
}
- (instancetype)initWithHandle:(Xm2vFence *)h device:(id)dev;
@end
@implementation XcmFence
- (instancetype)initWithHandle:(Xm2vFence *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_fence_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
@end

@interface XcmHeap : XcmObject {
@public
    id _device;
    NSUInteger _size;
    MTLStorageMode _storageMode;
    MTLCPUCacheMode _cpuCacheMode;
    MTLHazardTrackingMode _hazardTrackingMode;
    MTLHeapType _type;
    MTLPurgeableState _purgeable;
}
- (instancetype)initWithDevice:(id)dev descriptor:(MTLHeapDescriptor *)desc;
@end

@implementation XcmHeap
- (instancetype)initWithDevice:(id)dev descriptor:(MTLHeapDescriptor *)desc {
    if ((self = [super init])) {
        _device = dev;
        _size = desc.size;
        _storageMode = desc.storageMode;
        _cpuCacheMode = desc.cpuCacheMode;
        _hazardTrackingMode = desc.hazardTrackingMode;
        _type = desc.type;
        _purgeable = MTLPurgeableStateNonVolatile;
    }
    return self;
}
- (id<MTLDevice>)device { return _device; }
- (NSUInteger)size { return _size; }
- (NSUInteger)usedSize { return 0; }
- (NSUInteger)currentAllocatedSize { return _size; }
- (MTLStorageMode)storageMode { return _storageMode; }
- (MTLCPUCacheMode)cpuCacheMode { return _cpuCacheMode; }
- (MTLHazardTrackingMode)hazardTrackingMode { return _hazardTrackingMode; }
- (MTLResourceOptions)resourceOptions {
    return ((MTLResourceOptions)_storageMode << MTLResourceStorageModeShift) |
           ((MTLResourceOptions)_cpuCacheMode << MTLResourceCPUCacheModeShift) |
           ((MTLResourceOptions)_hazardTrackingMode << MTLResourceHazardTrackingModeShift);
}
- (MTLHeapType)type { return _type; }
- (NSUInteger)protectionOptions { return 0; }
- (NSUInteger)maxAvailableSizeWithAlignment:(NSUInteger)alignment { (void)alignment; return _size; }
- (MTLPurgeableState)setPurgeableState:(MTLPurgeableState)state {
    MTLPurgeableState prev = _purgeable;
    if (state != MTLPurgeableStateKeepCurrent) _purgeable = state;
    return prev;
}
- (id<MTLBuffer>)newBufferWithLength:(NSUInteger)length options:(MTLResourceOptions)options {
    return [_device newBufferWithLength:length options:options];
}
- (id<MTLBuffer>)newBufferWithLength:(NSUInteger)length options:(MTLResourceOptions)options offset:(NSUInteger)offset {
    (void)offset;
    return [_device newBufferWithLength:length options:options];
}
- (id<MTLTexture>)newTextureWithDescriptor:(MTLTextureDescriptor *)desc {
    return [_device newTextureWithDescriptor:desc];
}
- (id<MTLTexture>)newTextureWithDescriptor:(MTLTextureDescriptor *)desc offset:(NSUInteger)offset {
    (void)offset;
    return [_device newTextureWithDescriptor:desc];
}
@end

/* ======================== Forward declaration for CommandBuffer ======================== */

@interface XcmCommandBuffer : XcmObject {
@public
    Xm2vCommandBuffer *_handle;
    id _device;
    id _queue;
    NSMutableArray<MTLCommandBufferHandler> *_scheduledHandlers;
    NSMutableArray<MTLCommandBufferHandler> *_completedHandlers;
    NSMutableArray<XcmTexture *> *_dirtyIOSurfaceTextures;
    NSMutableArray *_retainedResources;
    dispatch_group_t _handlersGroup;
    pthread_t _handlersThread;
    BOOL _committed;
    BOOL _handlersFired;
    BOOL _iosurfaceFlushed;
    BOOL _retainedReleased;
    NSUInteger _protectionOptions;
}
- (instancetype)initWithHandle:(Xm2vCommandBuffer *)h device:(id)dev queue:(id)q;
- (void)markDirtyIOSurfaceTexture:(XcmTexture *)tex;
- (void)retainResource:(id)obj;
@end

/* ======================== Encoders ======================== */

@interface XcmComputeEncoder : XcmObject {
@public
    Xm2vComputeEncoder *_handle;
    id _device;
    XcmCommandBuffer *_cmdBuf;
    NSMutableArray *_retained;
    id _boundBuffers[31];
}
- (instancetype)initWithHandle:(Xm2vComputeEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb;
@end

@implementation XcmComputeEncoder
- (instancetype)initWithHandle:(Xm2vComputeEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _cmdBuf = cb;
        _retained = [NSMutableArray array];
    }
    return self;
}
- (void)dealloc {
    if (_handle) { xm2v_compute_encoder_end(_handle); _handle = nullptr; }
}
- (id<MTLDevice>)device { return _device; }
- (void)setComputePipelineState:(id<MTLComputePipelineState>)state {
    if (!_handle || !state || ![(id)state isKindOfClass:[XcmComputePipeline class]]) return;
    [_cmdBuf retainResource:state];
    xm2v_compute_encoder_set_pipeline(_handle, ((XcmComputePipeline *)state)->_handle);
}
- (void)setBuffer:(id<MTLBuffer>)buffer offset:(NSUInteger)offset atIndex:(NSUInteger)index {
    if (!_handle) return;
    if (index < 31) _boundBuffers[index] = buffer;
    Xm2vBuffer *bh = nullptr;
    if (buffer && [(id)buffer isKindOfClass:[XcmBuffer class]]) {
        [_cmdBuf retainResource:buffer];
        bh = ((XcmBuffer *)buffer)->_handle;
    }
    xm2v_compute_encoder_set_buffer(_handle, bh, offset, (uint32_t)index);
}
- (void)setBuffers:(const id<MTLBuffer> *)buffers offsets:(const NSUInteger *)offsets withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setBuffer:buffers[i] offset:offsets[i] atIndex:range.location + i];
}
- (void)setBufferOffset:(NSUInteger)offset atIndex:(NSUInteger)index {
    if (index < 31 && _boundBuffers[index])
        [self setBuffer:_boundBuffers[index] offset:offset atIndex:index];
}
- (void)setBytes:(const void *)bytes length:(NSUInteger)length atIndex:(NSUInteger)index {
    if (_handle) xm2v_compute_encoder_set_bytes(_handle, bytes, length, (uint32_t)index);
}
- (void)setTexture:(id<MTLTexture>)texture atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vTexture *th = nullptr;
    if (texture && [texture isKindOfClass:[XcmTexture class]]) {
        XcmTexture *xt = (XcmTexture *)texture;
        [_cmdBuf retainResource:xt];
        [xt syncFromIOSurfaceIfNeeded];
        if (xt->_iosurface && (xt->_usage & MTLTextureUsageShaderWrite))
            [_cmdBuf markDirtyIOSurfaceTexture:xt];
        th = xt->_handle;
    }
    xm2v_compute_encoder_set_texture(_handle, th, (uint32_t)index);
}
- (void)setTextures:(const id<MTLTexture> *)textures withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setTexture:textures[i] atIndex:range.location + i];
}
- (void)setSamplerState:(id<MTLSamplerState>)sampler atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = nullptr;
    if (sampler && [(id)sampler isKindOfClass:[XcmSampler class]]) {
        [_cmdBuf retainResource:sampler];
        sh = ((XcmSampler *)sampler)->_handle;
    }
    xm2v_compute_encoder_set_sampler(_handle, sh, (uint32_t)index);
}
- (void)setSamplerStates:(const id<MTLSamplerState> *)samplers withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setSamplerState:samplers[i] atIndex:range.location + i];
}
- (void)setThreadgroupMemoryLength:(NSUInteger)length atIndex:(NSUInteger)index {
    if (_handle) xm2v_compute_encoder_set_threadgroup_memory_length(_handle, (uint32_t)length, (uint32_t)index);
}
- (void)useResource:(id<MTLResource>)resource usage:(MTLResourceUsage)usage {
    if (!_handle || !resource) return;
    [_cmdBuf retainResource:resource];
    if ([resource isKindOfClass:[XcmBuffer class]]) {
        xm2v_compute_encoder_use_buffer(_handle, ((XcmBuffer *)resource)->_handle, (uint64_t)usage);
    } else if ([resource isKindOfClass:[XcmTexture class]]) {
        XcmTexture *xt = (XcmTexture *)resource;
        if (usage & (MTLResourceUsageRead | MTLResourceUsageSample)) [xt syncFromIOSurfaceIfNeeded];
        if ((usage & MTLResourceUsageWrite) && xt->_iosurface) [_cmdBuf markDirtyIOSurfaceTexture:xt];
        xm2v_compute_encoder_use_texture(_handle, xt->_handle, (uint64_t)usage);
    }
}
- (void)useResources:(const id<MTLResource> *)resources count:(NSUInteger)count usage:(MTLResourceUsage)usage {
    for (NSUInteger i = 0; i < count; i++)
        [self useResource:resources[i] usage:usage];
}
- (void)useHeap:(id<MTLHeap>)heap { (void)heap; }
- (void)useHeaps:(const id<MTLHeap> *)heaps count:(NSUInteger)count { (void)heaps; (void)count; }
- (void)dispatchThreadgroups:(MTLSize)threadgroupsPerGrid threadsPerThreadgroup:(MTLSize)threadsPerThreadgroup {
    if (_handle) {
        xm2v_compute_encoder_dispatch_threadgroups(_handle,
            threadgroupsPerGrid.width, threadgroupsPerGrid.height, threadgroupsPerGrid.depth,
            threadsPerThreadgroup.width, threadsPerThreadgroup.height, threadsPerThreadgroup.depth);
    }
}
- (void)dispatchThreads:(MTLSize)threadsPerGrid threadsPerThreadgroup:(MTLSize)threadsPerThreadgroup {
    if (_handle) {
        xm2v_compute_encoder_dispatch_threads(_handle,
            threadsPerGrid.width, threadsPerGrid.height, threadsPerGrid.depth,
            threadsPerThreadgroup.width, threadsPerThreadgroup.height, threadsPerThreadgroup.depth);
    }
}
- (void)updateFence:(id<MTLFence>)fence {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_compute_encoder_update_fence(_handle, ((XcmFence *)fence)->_handle);
}
- (void)waitForFence:(id<MTLFence>)fence {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_compute_encoder_wait_fence(_handle, ((XcmFence *)fence)->_handle);
}
- (void)endEncoding {
    if (_handle) { xm2v_compute_encoder_end(_handle); _handle = nullptr; }
    [_retained removeAllObjects];
}
- (void)pushDebugGroup:(NSString *)string { (void)string; }
- (void)popDebugGroup {}
- (void)insertDebugSignpost:(NSString *)string { (void)string; }
@end

@interface XcmRenderEncoder : XcmObject {
@public
    Xm2vRenderEncoder *_handle;
    id _device;
    XcmCommandBuffer *_cmdBuf;
    NSMutableArray *_retained;
    NSMutableDictionary *_vbufs, *_fbufs;   // slot -> bound buffer (for set*BufferOffset:atIndex:)
    BOOL _hasPipeline;
    XcmRenderPipeline *_curPso;
    uint32_t _tVbufMask, _tFbufMask, _tFtexMask, _tFsampMask;
    NSUInteger _tFbytesLen;
    uint32_t _tFbytesSlot;
    uint8_t _tFbytesHead[16];
    BOOL _tHasVp, _tHasScissor;
    MTLViewport _tVp;
    MTLScissorRect _tScissor;
}
- (instancetype)initWithHandle:(Xm2vRenderEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb;
@end

@implementation XcmRenderEncoder
- (instancetype)initWithHandle:(Xm2vRenderEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb {
    if ((self = [super init])) { _handle = h; _device = dev; _cmdBuf = cb; _retained = [NSMutableArray array]; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_render_encoder_end(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (void)setRenderPipelineState:(id<MTLRenderPipelineState>)pipelineState {
    if (!_handle) return;
    if (!pipelineState || ![pipelineState isKindOfClass:[XcmRenderPipeline class]] || !((XcmRenderPipeline *)pipelineState)->_handle) {
        _hasPipeline = NO;
        _curPso = nil;
        return;
    }
    _hasPipeline = YES;
    _curPso = (XcmRenderPipeline *)pipelineState;
    [_cmdBuf retainResource:pipelineState];
    bool ok = xm2v_render_encoder_set_pipeline(_handle, _curPso->_handle);
    if (!ok) fprintf(stderr, "XCM setRenderPipelineState FAILED: %s\n", xm2v_last_error());
}
- (void)setDepthStencilState:(id<MTLDepthStencilState>)depthStencilState {
    if (!_handle) return;
    Xm2vDepthStencil *h = nullptr;
    if (depthStencilState && [(id)depthStencilState isKindOfClass:[XcmDepthStencil class]]) {
        [_cmdBuf retainResource:depthStencilState];
        h = ((XcmDepthStencil *)depthStencilState)->_handle;
    }
    if (xcmTraceOn()) fprintf(stderr, "XCM setDepthStencil(%p)\n", (void *)h);
    xm2v_render_encoder_set_depth_stencil(_handle, h);
}
- (void)setViewport:(MTLViewport)vp {
    if (xcmTraceOn()) { _tHasVp = YES; _tVp = vp; }
    if (_handle) xm2v_render_encoder_set_viewport(_handle, vp.originX, vp.originY, vp.width, vp.height, vp.znear, vp.zfar);
}
- (void)setScissorRect:(MTLScissorRect)r {
    if (xcmTraceOn()) { _tHasScissor = YES; _tScissor = r; }
    if (_handle) xm2v_render_encoder_set_scissor(_handle, r.x, r.y, r.width, r.height);
}
- (void)setCullMode:(MTLCullMode)cullMode {
    if (_handle) if (xcmTraceOn()) fprintf(stderr, "XCM setCullMode(%lu)\n", (unsigned long)cullMode);
    xm2v_render_encoder_set_cull_mode(_handle, (uint64_t)cullMode);
}
- (void)setFrontFacingWinding:(MTLWinding)winding {
    if (_handle) xm2v_render_encoder_set_front_facing_winding(_handle, (uint64_t)winding);
}
- (void)setVertexBuffer:(id<MTLBuffer>)buffer offset:(NSUInteger)offset atIndex:(NSUInteger)index {
    if (!_handle) return;
    if (!_vbufs) _vbufs = [NSMutableDictionary dictionary];
    if (buffer) _vbufs[@(index)] = buffer; else [_vbufs removeObjectForKey:@(index)];
    Xm2vBuffer *bh = nullptr;
    if (buffer && [(id)buffer isKindOfClass:[XcmBuffer class]]) {
        [_cmdBuf retainResource:buffer];
        bh = ((XcmBuffer *)buffer)->_handle;
    }
    if (xcmTraceOn() && index < 32) { if (bh) _tVbufMask |= (1u << index); else _tVbufMask &= ~(1u << index); }
    xm2v_render_encoder_set_vertex_buffer(_handle, bh, offset, (uint32_t)index);
}
- (void)setVertexBytes:(const void *)bytes length:(NSUInteger)length atIndex:(NSUInteger)index {
    if (xcmTraceOn() && index < 32) _tVbufMask |= (1u << index);
    if (_handle) xm2v_render_encoder_set_vertex_bytes(_handle, bytes, length, (uint32_t)index);
}
- (void)setVertexTexture:(id<MTLTexture>)texture atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vTexture *th = nullptr;
    if (texture && [texture isKindOfClass:[XcmTexture class]]) {
        [_cmdBuf retainResource:texture];
        [(XcmTexture *)texture syncFromIOSurfaceIfNeeded];
        th = ((XcmTexture *)texture)->_handle;
    }
    xm2v_render_encoder_set_vertex_texture(_handle, th, (uint32_t)index);
}
- (void)setVertexSamplerState:(id<MTLSamplerState>)sampler atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = nullptr;
    if (sampler && [(id)sampler isKindOfClass:[XcmSampler class]]) {
        [_cmdBuf retainResource:sampler];
        sh = ((XcmSampler *)sampler)->_handle;
    }
    xm2v_render_encoder_set_vertex_sampler(_handle, sh, (uint32_t)index);
}
- (void)setFragmentBuffer:(id<MTLBuffer>)buffer offset:(NSUInteger)offset atIndex:(NSUInteger)index {
    if (!_handle) return;
    if (!_fbufs) _fbufs = [NSMutableDictionary dictionary];
    if (buffer) _fbufs[@(index)] = buffer; else [_fbufs removeObjectForKey:@(index)];
    Xm2vBuffer *bh = nullptr;
    if (buffer && [(id)buffer isKindOfClass:[XcmBuffer class]]) {
        [_cmdBuf retainResource:buffer];
        bh = ((XcmBuffer *)buffer)->_handle;
    }
    if (xcmTraceOn() && index < 32) { if (bh) _tFbufMask |= (1u << index); else _tFbufMask &= ~(1u << index); }
    if (!xm2v_render_encoder_set_fragment_buffer(_handle, bh, offset, (uint32_t)index))
        fprintf(stderr, "XCM setFragmentBuffer(%lu) FAILED: %s\n", (unsigned long)index, xm2v_last_error());
}
- (void)setFragmentBytes:(const void *)bytes length:(NSUInteger)length atIndex:(NSUInteger)index {
    if (xcmTraceOn()) {
        if (index < 32) _tFbufMask |= (1u << index);
        _tFbytesSlot = (uint32_t)index;
        _tFbytesLen = length;
        memset(_tFbytesHead, 0, sizeof(_tFbytesHead));
        if (bytes && length) memcpy(_tFbytesHead, bytes, length < 16 ? length : 16);
    }
    if (_handle) xm2v_render_encoder_set_fragment_bytes(_handle, bytes, length, (uint32_t)index);
}
- (void)setFragmentTexture:(id<MTLTexture>)texture atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vTexture *th = nullptr;
    if (texture && [texture isKindOfClass:[XcmTexture class]]) {
        [_cmdBuf retainResource:texture];
        [(XcmTexture *)texture syncFromIOSurfaceIfNeeded];
        th = ((XcmTexture *)texture)->_handle;
    }
    if (xcmTraceOn() && index < 32) { if (th) _tFtexMask |= (1u << index); else _tFtexMask &= ~(1u << index); }
    if (!xm2v_render_encoder_set_fragment_texture(_handle, th, (uint32_t)index))
        fprintf(stderr, "XCM setFragmentTexture(%lu) FAILED: %s\n", (unsigned long)index, xm2v_last_error());
}
- (void)setFragmentTextures:(const id<MTLTexture> *)textures withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setFragmentTexture:textures[i] atIndex:range.location + i];
}
- (void)setFragmentSamplerState:(id<MTLSamplerState>)sampler atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = nullptr;
    if (sampler && [(id)sampler isKindOfClass:[XcmSampler class]]) {
        [_cmdBuf retainResource:sampler];
        sh = ((XcmSampler *)sampler)->_handle;
    }
    if (xcmTraceOn() && index < 32) { if (sh) _tFsampMask |= (1u << index); else _tFsampMask &= ~(1u << index); }
    xm2v_render_encoder_set_fragment_sampler(_handle, sh, (uint32_t)index);
}
- (void)setFragmentSamplerStates:(const id<MTLSamplerState> *)samplers withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++)
        [self setFragmentSamplerState:samplers[i] atIndex:range.location + i];
}
- (void)setFragmentTexture:(id<MTLTexture>)texture atTextureIndex:(NSUInteger)texIdx
              samplerState:(id<MTLSamplerState>)sampler atSamplerIndex:(NSUInteger)sampIdx {
    [self setFragmentTexture:texture atIndex:texIdx];
    [self setFragmentSamplerState:sampler atIndex:sampIdx];
}
- (void)setFragmentSamplerState:(id<MTLSamplerState>)sampler lodMinClamp:(float)lodMin lodMaxClamp:(float)lodMax atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = nullptr;
    if (sampler && [(id)sampler isKindOfClass:[XcmSampler class]]) {
        [_cmdBuf retainResource:sampler];
        sh = ((XcmSampler *)sampler)->_handle;
    }
    if (xcmTraceOn() && index < 32) { if (sh) _tFsampMask |= (1u << index); else _tFsampMask &= ~(1u << index); }
    xm2v_render_encoder_set_fragment_sampler_lod(_handle, sh, lodMin, lodMax, (uint32_t)index);
}
- (void)setVertexSamplerState:(id<MTLSamplerState>)sampler lodMinClamp:(float)lodMin lodMaxClamp:(float)lodMax atIndex:(NSUInteger)index {
    if (!_handle) return;
    Xm2vSampler *sh = nullptr;
    if (sampler && [(id)sampler isKindOfClass:[XcmSampler class]]) {
        [_cmdBuf retainResource:sampler];
        sh = ((XcmSampler *)sampler)->_handle;
    }
    xm2v_render_encoder_set_vertex_sampler_lod(_handle, sh, lodMin, lodMax, (uint32_t)index);
}
- (void)setFragmentTexture:(id<MTLTexture>)texture atTextureIndex:(NSUInteger)texIdx
              samplerState:(id<MTLSamplerState>)sampler lodMinClamp:(float)lodMin lodMaxClamp:(float)lodMax
            atSamplerIndex:(NSUInteger)sampIdx {
    [self setFragmentTexture:texture atIndex:texIdx];
    [self setFragmentSamplerState:sampler lodMinClamp:lodMin lodMaxClamp:lodMax atIndex:sampIdx];
}
- (void)setVertexTexture:(id<MTLTexture>)texture atTextureIndex:(NSUInteger)texIdx
            samplerState:(id<MTLSamplerState>)sampler atSamplerIndex:(NSUInteger)sampIdx {
    [self setVertexTexture:texture atIndex:texIdx];
    [self setVertexSamplerState:sampler atIndex:sampIdx];
}
- (void)setVertexTexture:(id<MTLTexture>)texture atTextureIndex:(NSUInteger)texIdx
            samplerState:(id<MTLSamplerState>)sampler lodMinClamp:(float)lodMin lodMaxClamp:(float)lodMax
          atSamplerIndex:(NSUInteger)sampIdx {
    [self setVertexTexture:texture atIndex:texIdx];
    [self setVertexSamplerState:sampler lodMinClamp:lodMin lodMaxClamp:lodMax atIndex:sampIdx];
}
- (void)setVertexBufferOffset:(NSUInteger)offset atIndex:(NSUInteger)index {
    id b = _vbufs[@(index)]; if (b) [self setVertexBuffer:b offset:offset atIndex:index];
}
- (void)setFragmentBufferOffset:(NSUInteger)offset atIndex:(NSUInteger)index {
    id b = _fbufs[@(index)]; if (b) [self setFragmentBuffer:b offset:offset atIndex:index];
}
- (void)setVertexBuffers:(const id<MTLBuffer> *)buffers offsets:(const NSUInteger *)offsets withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++) [self setVertexBuffer:buffers[i] offset:offsets[i] atIndex:range.location + i];
}
- (void)setFragmentBuffers:(const id<MTLBuffer> *)buffers offsets:(const NSUInteger *)offsets withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++) [self setFragmentBuffer:buffers[i] offset:offsets[i] atIndex:range.location + i];
}
- (void)setVertexTextures:(const id<MTLTexture> *)textures withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++) [self setVertexTexture:textures[i] atIndex:range.location + i];
}
- (void)setVertexSamplerStates:(const id<MTLSamplerState> *)samplers withRange:(NSRange)range {
    for (NSUInteger i = 0; i < range.length; i++) [self setVertexSamplerState:samplers[i] atIndex:range.location + i];
}
- (void)setViewports:(const MTLViewport *)viewports count:(NSUInteger)count { if (count) [self setViewport:viewports[0]]; }
- (void)setScissorRects:(const MTLScissorRect *)rects count:(NSUInteger)count { if (count) [self setScissorRect:rects[0]]; }
// State the CPU backend keeps at its defaults (fill, clip, bias, blend constant, stencil reference) and residency hints.
- (void)setTriangleFillMode:(MTLTriangleFillMode)mode { (void)mode; }
- (void)setDepthClipMode:(MTLDepthClipMode)mode { (void)mode; }
- (void)setDepthBias:(float)b slopeScale:(float)s clamp:(float)c { (void)b; (void)s; (void)c; }
- (void)setBlendColorRed:(float)r green:(float)g blue:(float)b alpha:(float)a {
    if (_handle) xm2v_render_encoder_set_blend_color(_handle, r, g, b, a);
}
- (void)setStencilReferenceValue:(uint32_t)v {
    if (_handle) xm2v_render_encoder_set_stencil_reference_values(_handle, v, v);
}
- (void)setStencilFrontReferenceValue:(uint32_t)f backReferenceValue:(uint32_t)b {
    if (_handle) xm2v_render_encoder_set_stencil_reference_values(_handle, f, b);
}
- (void)setVisibilityResultMode:(MTLVisibilityResultMode)mode offset:(NSUInteger)offset { (void)mode; (void)offset; }
- (void)useResource:(id<MTLResource>)r usage:(MTLResourceUsage)u stages:(MTLRenderStages)st {
    if (!_handle || !r) return;
    [_cmdBuf retainResource:r];
    if ([r isKindOfClass:[XcmBuffer class]]) {
        xm2v_render_encoder_use_buffer(_handle, ((XcmBuffer *)r)->_handle, (uint64_t)u, (uint64_t)st);
    } else if ([r isKindOfClass:[XcmTexture class]]) {
        XcmTexture *xt = (XcmTexture *)r;
        if (u & (MTLResourceUsageRead | MTLResourceUsageSample)) [xt syncFromIOSurfaceIfNeeded];
        if ((u & MTLResourceUsageWrite) && xt->_iosurface) [_cmdBuf markDirtyIOSurfaceTexture:xt];
        xm2v_render_encoder_use_texture(_handle, xt->_handle, (uint64_t)u, (uint64_t)st);
    }
}
- (void)useResource:(id<MTLResource>)r usage:(MTLResourceUsage)u {
    [self useResource:r usage:u stages:(MTLRenderStageVertex | MTLRenderStageFragment)];
}
- (void)useResources:(const id<MTLResource> *)r count:(NSUInteger)n usage:(MTLResourceUsage)u stages:(MTLRenderStages)st {
    for (NSUInteger i = 0; i < n; i++) [self useResource:r[i] usage:u stages:st];
}
- (void)useResources:(const id<MTLResource> *)r count:(NSUInteger)n usage:(MTLResourceUsage)u {
    [self useResources:r count:n usage:u stages:(MTLRenderStageVertex | MTLRenderStageFragment)];
}
- (void)useHeap:(id<MTLHeap>)h { (void)h; }
- (void)useHeap:(id<MTLHeap>)h stages:(MTLRenderStages)st { (void)h; (void)st; }
- (void)useHeaps:(const id<MTLHeap> *)h count:(NSUInteger)n { (void)h; (void)n; }
- (void)useHeaps:(const id<MTLHeap> *)h count:(NSUInteger)n stages:(MTLRenderStages)st { (void)h; (void)n; (void)st; }
- (void)textureBarrier {}
- (void)memoryBarrierWithScope:(MTLBarrierScope)scope afterStages:(MTLRenderStages)after beforeStages:(MTLRenderStages)before {
    (void)scope; (void)after; (void)before;
}
- (void)memoryBarrierWithResources:(const id<MTLResource> *)resources count:(NSUInteger)count afterStages:(MTLRenderStages)after beforeStages:(MTLRenderStages)before {
    (void)resources; (void)count; (void)after; (void)before;
}
- (void)_traceDraw:(const char *)kind prim:(MTLPrimitiveType)prim start:(NSUInteger)start count:(NSUInteger)count inst:(NSUInteger)inst {
    if (!xcmTraceOn()) return;
    char fbHex[40] = "-";
    if (_tFbytesLen > 0) {
        size_t n = _tFbytesLen < 16 ? _tFbytesLen : 16;
        char *p = fbHex;
        for (size_t i = 0; i < n; i++) p += snprintf(p, 4, "%02x", _tFbytesHead[i]);
    }
    fprintf(stderr, "XCM %s(prim=%lu start=%lu cnt=%lu inst=%lu pso=%s/%s blend=%d mask=0x%llx vb=0x%x fb=0x%x ft=0x%x fs=0x%x fbytes[%u]=%lu:%s vp=%.0f,%.0f,%.0fx%.0f sc=%lu,%lu,%lux%lu)\n",
            kind, (unsigned long)prim, (unsigned long)start, (unsigned long)count, (unsigned long)inst,
            _curPso && _curPso->_vName ? _curPso->_vName.UTF8String : "?",
            _curPso && _curPso->_fName ? _curPso->_fName.UTF8String : "?",
            _curPso ? (int)_curPso->_blendEnabled : -1,
            _curPso ? (unsigned long long)_curPso->_writeMask : 0ull,
            _tVbufMask, _tFbufMask, _tFtexMask, _tFsampMask,
            _tFbytesSlot, (unsigned long)_tFbytesLen, fbHex,
            _tHasVp ? _tVp.originX : -1.0, _tHasVp ? _tVp.originY : -1.0,
            _tHasVp ? _tVp.width : 0.0, _tHasVp ? _tVp.height : 0.0,
            _tHasScissor ? (unsigned long)_tScissor.x : 0ul,
            _tHasScissor ? (unsigned long)_tScissor.y : 0ul,
            _tHasScissor ? (unsigned long)_tScissor.width : 0ul,
            _tHasScissor ? (unsigned long)_tScissor.height : 0ul);
}
- (void)drawPrimitives:(MTLPrimitiveType)primitiveType vertexStart:(NSUInteger)vertexStart
           vertexCount:(NSUInteger)vertexCount instanceCount:(NSUInteger)instanceCount
          baseInstance:(NSUInteger)baseInstance {
    if (_handle && _hasPipeline) {
        [self _traceDraw:"draw" prim:primitiveType start:vertexStart count:vertexCount inst:instanceCount];
        bool ok = xm2v_render_encoder_draw_primitives(_handle, (uint64_t)primitiveType, vertexStart, vertexCount, instanceCount, baseInstance);
        if (!ok) fprintf(stderr, "XCM drawPrimitives FAILED: %s\n", xm2v_last_error());
    }
}
- (void)drawPrimitives:(MTLPrimitiveType)primitiveType vertexStart:(NSUInteger)vertexStart vertexCount:(NSUInteger)vertexCount {
    [self drawPrimitives:primitiveType vertexStart:vertexStart vertexCount:vertexCount instanceCount:1 baseInstance:0];
}
- (void)drawIndexedPrimitives:(MTLPrimitiveType)primitiveType indexCount:(NSUInteger)indexCount
                    indexType:(MTLIndexType)indexType indexBuffer:(id<MTLBuffer>)indexBuffer
            indexBufferOffset:(NSUInteger)indexBufferOffset instanceCount:(NSUInteger)instanceCount
                   baseVertex:(NSInteger)baseVertex baseInstance:(NSUInteger)baseInstance {
    if (!_handle || !_hasPipeline || !indexBuffer || ![(id)indexBuffer isKindOfClass:[XcmBuffer class]]) return;
    [self _traceDraw:"drawIdx" prim:primitiveType start:indexBufferOffset count:indexCount inst:instanceCount];
    [_cmdBuf retainResource:indexBuffer];
    bool ok = xm2v_render_encoder_draw_indexed_primitives(_handle, (uint64_t)primitiveType, indexCount, (uint64_t)indexType,
        ((XcmBuffer *)indexBuffer)->_handle, indexBufferOffset, instanceCount, baseVertex, baseInstance);
    if (!ok) fprintf(stderr, "XCM drawIndexedPrimitives FAILED: %s\n", xm2v_last_error());
}
- (void)drawIndexedPrimitives:(MTLPrimitiveType)primitiveType indexCount:(NSUInteger)indexCount
                    indexType:(MTLIndexType)indexType indexBuffer:(id<MTLBuffer>)indexBuffer
            indexBufferOffset:(NSUInteger)indexBufferOffset {
    [self drawIndexedPrimitives:primitiveType indexCount:indexCount indexType:indexType
                    indexBuffer:indexBuffer indexBufferOffset:indexBufferOffset
                  instanceCount:1 baseVertex:0 baseInstance:0];
}
- (void)drawIndexedPrimitives:(MTLPrimitiveType)primitiveType indexCount:(NSUInteger)indexCount
                    indexType:(MTLIndexType)indexType indexBuffer:(id<MTLBuffer>)indexBuffer
            indexBufferOffset:(NSUInteger)indexBufferOffset instanceCount:(NSUInteger)instanceCount {
    [self drawIndexedPrimitives:primitiveType indexCount:indexCount indexType:indexType
                    indexBuffer:indexBuffer indexBufferOffset:indexBufferOffset
                  instanceCount:instanceCount baseVertex:0 baseInstance:0];
}
- (void)drawPrimitives:(MTLPrimitiveType)primitiveType vertexStart:(NSUInteger)vertexStart
           vertexCount:(NSUInteger)vertexCount instanceCount:(NSUInteger)instanceCount {
    [self drawPrimitives:primitiveType vertexStart:vertexStart vertexCount:vertexCount instanceCount:instanceCount baseInstance:0];
}
- (void)updateFence:(id<MTLFence>)fence afterStages:(MTLRenderStages)stages {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_render_encoder_update_fence(_handle, ((XcmFence *)fence)->_handle, (uint64_t)stages);
}
- (void)waitForFence:(id<MTLFence>)fence beforeStages:(MTLRenderStages)stages {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_render_encoder_wait_fence(_handle, ((XcmFence *)fence)->_handle, (uint64_t)stages);
}
- (void)endEncoding {
    if (_handle) {
        bool ok = xm2v_render_encoder_end(_handle);
        if (!ok) fprintf(stderr, "XCM render endEncoding FAILED: %s\n", xm2v_last_error());
        _handle = nullptr;
    }
    [_retained removeAllObjects];
}
- (void)pushDebugGroup:(NSString *)string { (void)string; }
- (void)popDebugGroup {}
- (void)insertDebugSignpost:(NSString *)string { (void)string; }
@end

@interface XcmBlitEncoder : XcmObject {
@public
    Xm2vBlitEncoder *_handle;
    id _device;
    XcmCommandBuffer *_cmdBuf;
    NSMutableArray *_retained;
}
- (instancetype)initWithHandle:(Xm2vBlitEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb;
@end

@implementation XcmBlitEncoder
- (instancetype)initWithHandle:(Xm2vBlitEncoder *)h device:(id)dev commandBuffer:(XcmCommandBuffer *)cb {
    if ((self = [super init])) { _handle = h; _device = dev; _cmdBuf = cb; _retained = [NSMutableArray array]; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_blit_encoder_end(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (void)copyFromBuffer:(id<MTLBuffer>)src sourceOffset:(NSUInteger)srcOff
              toBuffer:(id<MTLBuffer>)dst destinationOffset:(NSUInteger)dstOff size:(NSUInteger)size {
    if (!_handle || !src || !dst || ![(id)src isKindOfClass:[XcmBuffer class]] || ![(id)dst isKindOfClass:[XcmBuffer class]]) return;
    [_cmdBuf retainResource:src]; [_cmdBuf retainResource:dst];
    xm2v_blit_encoder_copy_buffer_to_buffer(_handle, ((XcmBuffer *)src)->_handle, srcOff, ((XcmBuffer *)dst)->_handle, dstOff, size);
}
- (void)copyFromBuffer:(id<MTLBuffer>)src sourceOffset:(NSUInteger)srcOff
     sourceBytesPerRow:(NSUInteger)bpr sourceBytesPerImage:(NSUInteger)bpi
            sourceSize:(MTLSize)sz toTexture:(id<MTLTexture>)dst
      destinationSlice:(NSUInteger)slice destinationLevel:(NSUInteger)level destinationOrigin:(MTLOrigin)origin {
    if (!_handle || !src || !dst || ![(id)src isKindOfClass:[XcmBuffer class]] || ![(id)dst isKindOfClass:[XcmTexture class]]) return;
    [_cmdBuf retainResource:src]; [_cmdBuf retainResource:dst];
    if (((XcmTexture *)dst)->_iosurface) {
        [(XcmTexture *)dst syncFromIOSurfaceIfNeeded];   // the copy may cover a part; the write-back covers all
        [_cmdBuf markDirtyIOSurfaceTexture:(XcmTexture *)dst];
    }
    xm2v_blit_encoder_copy_buffer_to_texture(_handle, ((XcmBuffer *)src)->_handle, srcOff, bpr, bpi,
        sz.width, sz.height, sz.depth, ((XcmTexture *)dst)->_handle, slice, level, origin.x, origin.y, origin.z);
}
- (void)copyFromTexture:(id<MTLTexture>)src sourceSlice:(NSUInteger)slice sourceLevel:(NSUInteger)level
           sourceOrigin:(MTLOrigin)origin sourceSize:(MTLSize)sz toBuffer:(id<MTLBuffer>)dst
      destinationOffset:(NSUInteger)dstOff destinationBytesPerRow:(NSUInteger)bpr destinationBytesPerImage:(NSUInteger)bpi {
    if (!_handle || !src || !dst || ![(id)src isKindOfClass:[XcmTexture class]] || ![(id)dst isKindOfClass:[XcmBuffer class]]) return;
    [_cmdBuf retainResource:src]; [_cmdBuf retainResource:dst];
    [(XcmTexture *)src syncFromIOSurfaceIfNeeded];
    xm2v_blit_encoder_copy_texture_to_buffer(_handle, ((XcmTexture *)src)->_handle, slice, level,
        origin.x, origin.y, origin.z, sz.width, sz.height, sz.depth, ((XcmBuffer *)dst)->_handle, dstOff, bpr, bpi);
}
- (void)copyFromTexture:(id<MTLTexture>)src sourceSlice:(NSUInteger)ss sourceLevel:(NSUInteger)sl
           sourceOrigin:(MTLOrigin)so sourceSize:(MTLSize)sz toTexture:(id<MTLTexture>)dst
       destinationSlice:(NSUInteger)ds destinationLevel:(NSUInteger)dl destinationOrigin:(MTLOrigin)do_ {
    if (!_handle || !src || !dst || ![(id)src isKindOfClass:[XcmTexture class]] || ![(id)dst isKindOfClass:[XcmTexture class]]) return;
    [_cmdBuf retainResource:src]; [_cmdBuf retainResource:dst];
    [(XcmTexture *)src syncFromIOSurfaceIfNeeded];
    if (((XcmTexture *)dst)->_iosurface) {
        [(XcmTexture *)dst syncFromIOSurfaceIfNeeded];   // the copy may cover a part; the write-back covers all
        [_cmdBuf markDirtyIOSurfaceTexture:(XcmTexture *)dst];
    }
    xm2v_blit_encoder_copy_texture_to_texture(_handle, ((XcmTexture *)src)->_handle, ss, sl,
        so.x, so.y, so.z, sz.width, sz.height, sz.depth, ((XcmTexture *)dst)->_handle, ds, dl, do_.x, do_.y, do_.z);
}
- (void)fillBuffer:(id<MTLBuffer>)buffer range:(NSRange)range value:(uint8_t)value {
    if (!_handle || !buffer || ![(id)buffer isKindOfClass:[XcmBuffer class]]) return;
    [_cmdBuf retainResource:buffer];
    xm2v_blit_encoder_fill_buffer(_handle, ((XcmBuffer *)buffer)->_handle, range.location, range.length, value);
}
- (void)generateMipmapsForTexture:(id<MTLTexture>)texture {
    if (!_handle || !texture || ![(id)texture isKindOfClass:[XcmTexture class]]) return;
    [_cmdBuf retainResource:texture];
    xm2v_blit_encoder_generate_mipmaps(_handle, ((XcmTexture *)texture)->_handle);
}
- (void)synchronizeResource:(id<MTLResource>)resource {
    if (!_handle || !resource) return;
    [_cmdBuf retainResource:resource];
    if ([resource isKindOfClass:[XcmBuffer class]])
        xm2v_blit_encoder_synchronize_buffer(_handle, ((XcmBuffer *)resource)->_handle);
    else if ([resource isKindOfClass:[XcmTexture class]])
        xm2v_blit_encoder_synchronize_texture(_handle, ((XcmTexture *)resource)->_handle);
}
- (void)updateFence:(id<MTLFence>)fence {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_blit_encoder_update_fence(_handle, ((XcmFence *)fence)->_handle);
}
- (void)waitForFence:(id<MTLFence>)fence {
    if (!_handle || !fence || ![(id)fence isKindOfClass:[XcmFence class]]) return;
    [_cmdBuf retainResource:fence];
    xm2v_blit_encoder_wait_fence(_handle, ((XcmFence *)fence)->_handle);
}
- (void)endEncoding {
    if (_handle) { xm2v_blit_encoder_end(_handle); _handle = nullptr; }
    [_retained removeAllObjects];
}
- (void)pushDebugGroup:(NSString *)string { (void)string; }
- (void)popDebugGroup {}
- (void)insertDebugSignpost:(NSString *)string { (void)string; }
@end

/* ======================== CommandBuffer & Queue ======================== */

@implementation XcmCommandBuffer
- (instancetype)initWithHandle:(Xm2vCommandBuffer *)h device:(id)dev queue:(id)q {
    if ((self = [super init])) {
        _handle = h;
        _device = dev;
        _queue = q;
        _scheduledHandlers = [NSMutableArray array];
        _completedHandlers = [NSMutableArray array];
        _dirtyIOSurfaceTextures = [NSMutableArray array];
        _retainedResources = [NSMutableArray array];
        _handlersGroup = dispatch_group_create();
    }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_command_buffer_release(_handle); _handle = nullptr; } }
- (void)retainResource:(id)obj {
    if (!obj) return;
    @synchronized(self) {
        if (!_retainedReleased) [_retainedResources addObject:obj];
    }
}
- (void)markDirtyIOSurfaceTexture:(XcmTexture *)tex {
    if (!tex || !tex->_iosurface) return;
    BOOL added = NO;
    @synchronized(self) {
        if (!_iosurfaceFlushed && ![_dirtyIOSurfaceTextures containsObject:tex]) {
            [_dirtyIOSurfaceTextures addObject:tex];
            added = YES;
        }
    }
    if (added) xcmRecordIOSurfaceWriter(tex->_iosurface, self);
}
- (id<MTLDevice>)device { return _device; }
- (id<MTLCommandQueue>)commandQueue { return _queue; }
- (BOOL)retainedReferences { return YES; }
- (NSUInteger)protectionOptions { return _protectionOptions; }
- (void)setProtectionOptions:(NSUInteger)opts { _protectionOptions = opts; }
- (CFTimeInterval)GPUStartTime { return 0.0; }
- (CFTimeInterval)GPUEndTime { return 0.0; }
- (CFTimeInterval)kernelStartTime { return 0.0; }
- (CFTimeInterval)kernelEndTime { return 0.0; }
- (MTLCommandBufferStatus)status {
    return _handle ? (MTLCommandBufferStatus)xm2v_command_buffer_status(_handle) : MTLCommandBufferStatusError;
}
- (NSError *)error {
    if (self.status == MTLCommandBufferStatusError) {
        // This command buffer's own text. Not xcmError: it prefers the thread's last bridge error, which is whatever
        // call failed last on this thread (a function a library does not have, long ago), not this failure.
        char buf[512] = {};
        NSString *text = nil;
        if (_handle && xm2v_command_buffer_error(_handle, buf, sizeof(buf)) > 0) {
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: command buffer error (cb=%p): %{public}s", (__bridge void *)self, buf);
            text = [NSString stringWithUTF8String:buf];
        }
        return [NSError errorWithDomain:MTLCommandBufferErrorDomain code:MTLCommandBufferErrorInternal
                               userInfo:@{NSLocalizedDescriptionKey: text ?: @"Command buffer failed"}];
    }
    return nil;
}
- (void)enqueue {}
- (void)addScheduledHandler:(MTLCommandBufferHandler)block {
    if (!block) return;
    @synchronized(self) {
        if (!_handlersFired) [_scheduledHandlers addObject:[block copy]];
    }
}
- (void)addCompletedHandler:(MTLCommandBufferHandler)block {
    if (!block) return;
    @synchronized(self) {
        if (!_handlersFired) [_completedHandlers addObject:[block copy]];
    }
}
- (void)_flushIOSurfacesOnce {
    @synchronized(self) {
        if (_iosurfaceFlushed) return;
        _iosurfaceFlushed = YES;
        for (XcmTexture *tex in _dirtyIOSurfaceTextures) [tex syncToIOSurface];
        [_dirtyIOSurfaceTextures removeAllObjects];
    }
}
- (void)_fireHandlersOnce {
    NSArray<MTLCommandBufferHandler> *sched = nil;
    NSArray<MTLCommandBufferHandler> *comp = nil;
    BOOL shouldRun = NO;
    @synchronized(self) {
        if (!_handlersFired) {
            _handlersFired = YES;
            sched = [_scheduledHandlers copy];
            comp = [_completedHandlers copy];
            [_scheduledHandlers removeAllObjects];
            [_completedHandlers removeAllObjects];
            if (sched.count || comp.count) {
                shouldRun = YES;
                _handlersThread = pthread_self();
                dispatch_group_enter(_handlersGroup);
            }
        }
    }
    if (shouldRun) {
        for (MTLCommandBufferHandler h in sched) h((id<MTLCommandBuffer>)self);
        for (MTLCommandBufferHandler h in comp) h((id<MTLCommandBuffer>)self);
        @synchronized(self) {
            _handlersThread = nullptr;
        }
        dispatch_group_leave(_handlersGroup);
    } else {
        BOOL isSameThread = NO;
        @synchronized(self) {
            isSameThread = (_handlersThread != nullptr && pthread_equal(_handlersThread, pthread_self()) != 0);
        }
        if (!isSameThread) {
            dispatch_group_wait(_handlersGroup, DISPATCH_TIME_FOREVER);
        }
    }
}
- (void)_releaseRetainedResourcesOnce {
    NSArray *toRelease = nil;
    @synchronized(self) {
        if (_retainedReleased) return;
        _retainedReleased = YES;
        toRelease = [_retainedResources copy];
        [_retainedResources removeAllObjects];
    }
    if (toRelease.count && _handle &&
        (MTLCommandBufferStatus)xm2v_command_buffer_status(_handle) == MTLCommandBufferStatusError) {
        char ebuf[256] = {};
        if (xm2v_command_buffer_error(_handle, ebuf, sizeof(ebuf)) > 0 &&
            (strstr(ebuf, "GpuHang") || strstr(ebuf, "DeviceLost"))) {
            // Driver may still be executing in-flight draws on a parked device; keep resources alive
            // rather than running fallback deallocators or freeing backing memory under lavapipe.
            static NSMutableArray *gParkedResources;
            static dispatch_once_t once;
            dispatch_once(&once, ^{ gParkedResources = [NSMutableArray array]; });
            @synchronized(gParkedResources) { [gParkedResources addObjectsFromArray:toRelease]; }
            return;
        }
    }
    (void)toRelease;
}
- (void)commit {
    BOOL needsWait = NO;
    NSUInteger nSched = 0, nComp = 0, nDirty = 0, nRet = 0;
    NSArray *retSnapshot = nil;
    @synchronized(self) {
        if (_committed) return;
        _committed = YES;
        nSched = _scheduledHandlers.count;
        nComp = _completedHandlers.count;
        nDirty = _dirtyIOSurfaceTextures.count;
        nRet = _retainedResources.count;
        if (nRet) retSnapshot = [_retainedResources copy];
        needsWait = (nSched || nComp || nDirty || nRet);
    }
    for (id res in retSnapshot) {
        if ([res isKindOfClass:[XcmBuffer class]]) {
            [(XcmBuffer *)res syncFromFallbackHostMemory];
        } else if ([res isKindOfClass:[XcmTexture class]]) {
            XcmBuffer *b = ((XcmTexture *)res)->_buffer;
            if (b) [b syncFromFallbackHostMemory];
        }
    }
    if (xcmTraceOn()) fprintf(stderr, "XCM commit cb=%p sched=%lu comp=%lu dirty=%lu ret=%lu\n",
                              (__bridge void *)self, (unsigned long)nSched,
                              (unsigned long)nComp, (unsigned long)nDirty, (unsigned long)nRet);
    if (_handle) {
        bool ok = xm2v_command_buffer_commit(_handle);
        if (!ok) fprintf(stderr, "XCM commit FAILED cb=%p err=%s\n", (__bridge void *)self, xm2v_last_error());
    }
    if (needsWait) {
        static dispatch_queue_t waitQ;
        static dispatch_once_t once;
        dispatch_once(&once, ^{ waitQ = dispatch_queue_create("xcm.cb.wait", DISPATCH_QUEUE_CONCURRENT); });
        dispatch_async(waitQ, ^{
            if (_handle) {
                bool wok = xm2v_command_buffer_wait_until_completed(_handle, 0);
                uint32_t st = xm2v_command_buffer_status(_handle);
                if (!wok || xcmTraceOn()) {
                    char ebuf[256] = {};
                    if (!wok) xm2v_command_buffer_error(_handle, ebuf, sizeof(ebuf));
                    fprintf(stderr, "XCM cb-done cb=%p wok=%d status=%u err=%s\n",
                            (__bridge void *)self, wok, st, ebuf[0] ? ebuf : (wok ? "ok" : xm2v_last_error()));
                }
            }
            [self _flushIOSurfacesOnce];
            [self _fireHandlersOnce];
            [self _releaseRetainedResourcesOnce];
        });
    }
}
- (void)commitAndWaitUntilSubmitted {
    [self commit];
}
- (void)waitUntilScheduled {
    [self waitUntilCompleted];
}
- (void)waitUntilCompleted {
    if (_handle) xm2v_command_buffer_wait_until_completed(_handle, 0);
    [self _flushIOSurfacesOnce];
    [self _fireHandlersOnce];
    [self _releaseRetainedResourcesOnce];
}
- (id<MTLComputeCommandEncoder>)computeCommandEncoder {
    if (!_handle) return nil;
    Xm2vComputeEncoder *eh = xm2v_compute_encoder_new(_handle);
    if (xcmTraceOn()) fprintf(stderr, "XCM computeCommandEncoder -> %p\n", (void *)eh);
    return eh ? (id<MTLComputeCommandEncoder>)[[XcmComputeEncoder alloc] initWithHandle:eh device:_device commandBuffer:self] : nil;
}
- (id<MTLComputeCommandEncoder>)computeCommandEncoderWithDispatchType:(MTLDispatchType)dispatchType {
    (void)dispatchType;
    return [self computeCommandEncoder];
}
- (id<MTLRenderCommandEncoder>)renderCommandEncoderWithDescriptor:(MTLRenderPassDescriptor *)desc {
    if (!_handle || !desc) return nil;
    Xm2vRenderPassDesc rd = {};
    rd.color_attachment_count = 8;
    for (int i = 0; i < 8; i++) {
        MTLRenderPassColorAttachmentDescriptor *ca = desc.colorAttachments[i];
        if (ca && ca.texture && [ca.texture isKindOfClass:[XcmTexture class]]) {
            XcmTexture *xt = (XcmTexture *)ca.texture;
            [self retainResource:xt];
            if (ca.loadAction == MTLLoadActionLoad) [xt syncFromIOSurfaceIfNeeded];
            if (xt->_iosurface && ca.storeAction != MTLStoreActionDontCare)
                [self markDirtyIOSurfaceTexture:xt];
            rd.color_attachments[i].texture = xt->_handle;
            rd.color_attachments[i].level = ca.level;
            rd.color_attachments[i].slice = ca.slice;
            rd.color_attachments[i].load_action = (uint64_t)ca.loadAction;
            rd.color_attachments[i].store_action = (uint64_t)ca.storeAction;
            rd.color_attachments[i].clear_r = ca.clearColor.red;
            rd.color_attachments[i].clear_g = ca.clearColor.green;
            rd.color_attachments[i].clear_b = ca.clearColor.blue;
            rd.color_attachments[i].clear_a = ca.clearColor.alpha;
        }
    }
    if (desc.depthAttachment.texture && [desc.depthAttachment.texture isKindOfClass:[XcmTexture class]]) {
        [self retainResource:desc.depthAttachment.texture];
        rd.depth_texture = ((XcmTexture *)desc.depthAttachment.texture)->_handle;
        rd.depth_load_action = (uint64_t)desc.depthAttachment.loadAction;
        rd.depth_store_action = (uint64_t)desc.depthAttachment.storeAction;
        rd.clear_depth = desc.depthAttachment.clearDepth;
    }
    if (desc.stencilAttachment.texture && [desc.stencilAttachment.texture isKindOfClass:[XcmTexture class]]) {
        [self retainResource:desc.stencilAttachment.texture];
        rd.stencil_texture = ((XcmTexture *)desc.stencilAttachment.texture)->_handle;
        rd.stencil_load_action = (uint64_t)desc.stencilAttachment.loadAction;
        rd.stencil_store_action = (uint64_t)desc.stencilAttachment.storeAction;
        rd.clear_stencil = desc.stencilAttachment.clearStencil;
    }
    Xm2vRenderEncoder *eh = xm2v_render_encoder_new(_handle, &rd);
    if (xcmTraceOn()) {
        MTLRenderPassColorAttachmentDescriptor *ca0 = desc.colorAttachments[0];
        id t0 = ca0 ? ca0.texture : nil;
        fprintf(stderr, "XCM renderEncoder -> %p c0=%p(%lux%lu load=%lu store=%lu clear=%.2f,%.2f,%.2f,%.2f) d=%p s=%p\n",
                (void *)eh, (__bridge void *)t0,
                t0 ? (unsigned long)[t0 width] : 0ul, t0 ? (unsigned long)[t0 height] : 0ul,
                ca0 ? (unsigned long)ca0.loadAction : 0ul, ca0 ? (unsigned long)ca0.storeAction : 0ul,
                ca0 ? ca0.clearColor.red : 0.0, ca0 ? ca0.clearColor.green : 0.0,
                ca0 ? ca0.clearColor.blue : 0.0, ca0 ? ca0.clearColor.alpha : 0.0, (__bridge void *)desc.depthAttachment.texture, (__bridge void *)desc.stencilAttachment.texture);
    }
    return eh ? (id<MTLRenderCommandEncoder>)[[XcmRenderEncoder alloc] initWithHandle:eh device:_device commandBuffer:self] : nil;
}
- (id<MTLBlitCommandEncoder>)blitCommandEncoder {
    if (!_handle) return nil;
    Xm2vBlitEncoder *eh = xm2v_blit_encoder_new(_handle);
    return eh ? (id<MTLBlitCommandEncoder>)[[XcmBlitEncoder alloc] initWithHandle:eh device:_device commandBuffer:self] : nil;
}
- (void)encodeSignalEvent:(id<MTLEvent>)event value:(uint64_t)value {
    if (!_handle || !event) return;
    [self retainResource:event];
    if ([event isKindOfClass:[XcmSharedEvent class]]) {
        if (!xm2v_command_buffer_encode_signal_shared_event(_handle, ((XcmSharedEvent *)event)->_handle, value))
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: encodeSignalEvent(shared) failed: %{public}s", xm2v_last_error() ?: "?");
    } else if ([event isKindOfClass:[XcmEvent class]]) {
        if (!xm2v_command_buffer_encode_signal_event(_handle, ((XcmEvent *)event)->_handle, value))
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: encodeSignalEvent failed: %{public}s", xm2v_last_error() ?: "?");
    }
}
- (void)encodeSignalEventScheduled:(id<MTLEvent>)event value:(uint64_t)value {
    if (!_handle || !event) return;
    [self retainResource:event];
    if ([event isKindOfClass:[XcmSharedEvent class]]) {
        xm2v_command_buffer_encode_signal_shared_event_scheduled(_handle, ((XcmSharedEvent *)event)->_handle, value);
    } else if ([event isKindOfClass:[XcmEvent class]]) {
        xm2v_command_buffer_encode_signal_event_scheduled(_handle, ((XcmEvent *)event)->_handle, value);
    }
}
- (void)encodeWaitForEvent:(id<MTLEvent>)event value:(uint64_t)value {
    if (!_handle || !event) return;
    [self retainResource:event];
    if ([event isKindOfClass:[XcmSharedEvent class]]) {
        if (!xm2v_command_buffer_encode_wait_shared_event(_handle, ((XcmSharedEvent *)event)->_handle, value))
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: encodeWaitForEvent(shared) failed: %{public}s", xm2v_last_error() ?: "?");
    } else if ([event isKindOfClass:[XcmEvent class]]) {
        if (!xm2v_command_buffer_encode_wait_event(_handle, ((XcmEvent *)event)->_handle, value))
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: encodeWaitForEvent failed: %{public}s", xm2v_last_error() ?: "?");
    }
}
- (void)pushDebugGroup:(NSString *)string { (void)string; }
- (void)popDebugGroup {}
@end

@interface XcmQueue : XcmObject {
@public
    Xm2vQueue *_handle;
    id _device;
    NSUInteger _gpuPriority;
    NSUInteger _bgGpuPriority;
    dispatch_queue_t _submissionQueue;
    dispatch_queue_t _completionQueue;
}
- (instancetype)initWithHandle:(Xm2vQueue *)h device:(id)dev;
@end

@implementation XcmQueue
- (instancetype)initWithHandle:(Xm2vQueue *)h device:(id)dev {
    if ((self = [super init])) { _handle = h; _device = dev; _gpuPriority = 1; _bgGpuPriority = 2; }
    return self;
}
- (void)dealloc { if (_handle) { xm2v_queue_release(_handle); _handle = nullptr; } }
- (id<MTLDevice>)device { return _device; }
- (void)setSubmissionQueue:(dispatch_queue_t)q { _submissionQueue = q; }
- (void)setCompletionQueue:(dispatch_queue_t)q { _completionQueue = q; }
- (NSUInteger)getGPUPriority { return _gpuPriority; }
- (NSUInteger)getBackgroundGPUPriority { return _bgGpuPriority; }
- (BOOL)setGPUPriority:(NSUInteger)p { _gpuPriority = p; return YES; }
- (BOOL)setGPUPriority:(NSUInteger)p offset:(unsigned short)o { (void)o; _gpuPriority = p; return YES; }
- (BOOL)setBackgroundGPUPriority:(NSUInteger)p { _bgGpuPriority = p; return YES; }
- (BOOL)setBackgroundGPUPriority:(NSUInteger)p offset:(unsigned short)o { (void)o; _bgGpuPriority = p; return YES; }
- (BOOL)_setGPUPriority:(NSUInteger)p backgroundPriority:(NSUInteger)b { _gpuPriority = p; _bgGpuPriority = b; return YES; }
- (id<MTLCommandBuffer>)commandBuffer {
    if (!_handle) return nil;
    Xm2vCommandBuffer *cb = xm2v_command_buffer_new(_handle);
    return cb ? (id<MTLCommandBuffer>)[[XcmCommandBuffer alloc] initWithHandle:cb device:_device queue:self] : nil;
}
- (id<MTLCommandBuffer>)commandBufferWithUnretainedReferences { return [self commandBuffer]; }
- (id<MTLCommandBuffer>)commandBufferWithDescriptor:(MTLCommandBufferDescriptor *)desc {
    (void)desc;
    return [self commandBuffer];
}
@end

/* ======================== Dynamic _MTLDevice Subclass ======================== */

static Xm2vDevice *gXm2vDevice = nullptr;
static uint64_t gRegistryID = 0x584e554350550001ULL;
static id gSingletonDevice = nil;

static NSString *xcmDev_name(id, SEL) {
    char buf[256] = {};
    if (gXm2vDevice && xm2v_device_name(gXm2vDevice, buf, sizeof(buf)) > 0)
        return [NSString stringWithUTF8String:buf];
    return @"XnuMetal";
}
static uint64_t xcmDev_registryID(id, SEL) { return gRegistryID; }
static BOOL xcmDev_isLowPower(id, SEL) { return NO; }
static BOOL xcmDev_isHeadless(id, SEL) { return NO; }
static BOOL xcmDev_isRemovable(id, SEL) { return NO; }
static BOOL xcmDev_hasUnifiedMemory(id, SEL) { return YES; }
static NSUInteger xcmDev_maxBufferLength(id, SEL) {
    return gXm2vDevice ? (NSUInteger)xm2v_device_max_buffer_length(gXm2vDevice) : (256 * 1024 * 1024);
}
static MTLSize xcmDev_maxThreadsPerThreadgroup(id, SEL) {
    uint64_t v[3] = {256, 256, 256};
    if (gXm2vDevice) xm2v_device_max_threads_per_threadgroup(gXm2vDevice, v);
    return MTLSizeMake((NSUInteger)v[0], (NSUInteger)v[1], (NSUInteger)v[2]);
}
static NSUInteger xcmDev_maxTex2D(id, SEL) { return 16384; }
static NSUInteger xcmDev_maxTex3D(id, SEL) { return 2048; }
static NSUInteger xcmDev_maxTexLayers(id, SEL) { return 2048; }
static NSUInteger xcmDev_maxColorAtt(id, SEL) { return 8; }
static NSUInteger xcmDev_maxTGMem(id, SEL) { return 32768; }
static NSUInteger xcmDev_minLinearAlign(id, SEL, MTLPixelFormat) { return 16; }
static NSUInteger xcmDev_minTileAlign(id, SEL, MTLPixelFormat) { return 16; }
static NSUInteger xcmDev_align16(id, SEL) { return 16; }
static NSUInteger xcmDev_rwTexTier(id, SEL) { return MTLReadWriteTextureTier2; }
static BOOL xcmDev_yes(id, SEL) { return YES; }
static BOOL xcmDev_supportsSampleCount(id, SEL, NSUInteger c) { return c == 1; }
static BOOL xcmDev_supportsFeatureSet(id, SEL, NSUInteger) { return YES; }
// A device's families do not change, and the bridge audits its whole format table to answer for one. Metal asks
// several times for every MTLPixelFormatGetInfoForDevice, which CoreAnimation calls for each surface it makes: half of
// WindowServer's time in a display update went there. Each answer is kept (per device, a handful of families).
static bool xcmBridgeSupportsFamily(int64_t family) {
    static os_unfair_lock lock = OS_UNFAIR_LOCK_INIT;
    static const void *dev;
    static struct { int64_t family; bool yes; } known[32];
    static unsigned n;
    const void *d = gXm2vDevice;
    if (!d) return false;
    os_unfair_lock_lock(&lock);
    if (dev != d) { dev = d; n = 0; }
    for (unsigned i = 0; i < n; i++) {
        if (known[i].family == family) { bool yes = known[i].yes; os_unfair_lock_unlock(&lock); return yes; }
    }
    os_unfair_lock_unlock(&lock);
    bool yes = xm2v_device_supports_family(gXm2vDevice, family);
    os_unfair_lock_lock(&lock);
    if (dev == d && n < sizeof(known) / sizeof(known[0])) { known[n].family = family; known[n].yes = yes; n++; }
    os_unfair_lock_unlock(&lock);
    return yes;
}
static BOOL xcmDev_supportsFamily(id, SEL, MTLGPUFamily family) {
    if (xcmBridgeSupportsFamily((int64_t)family)) return YES;
    // Report Apple1/Apple2, Mac1/Mac2, Common1..3 so QuartzCore enables framebuffer fetch (programmable blending)
    // and standard macOS/Apple GPU paths that lavapipe + metal2vulkan support.
    switch ((int64_t)family) {
        case 1001: case 1002:             // MTLGPUFamilyApple1..Apple2
        case 2001: case 2002:             // MTLGPUFamilyMac1..Mac2
        case 3001: case 3002: case 3003:  // MTLGPUFamilyCommon1..Common3
            return YES;
        default:
            return NO;
    }
}

static id xcmDev_newCommandQueue(id self, SEL) NS_RETURNS_RETAINED;
static id xcmDev_newCommandQueueMax(id self, SEL, NSUInteger) NS_RETURNS_RETAINED;
static id xcmDev_newBufferLen(id self, SEL, NSUInteger, MTLResourceOptions) NS_RETURNS_RETAINED;
static id xcmDev_newBufferBytes(id self, SEL, const void *, NSUInteger, MTLResourceOptions) NS_RETURNS_RETAINED;
static id xcmDev_newBufferNoCopy(id self, SEL, void *, NSUInteger, MTLResourceOptions, void (^)(void *, NSUInteger)) NS_RETURNS_RETAINED;
static id xcmDev_newTexture(id self, SEL, MTLTextureDescriptor *) NS_RETURNS_RETAINED;
static id xcmDev_newTextureIOSurface(id self, SEL, MTLTextureDescriptor *, IOSurfaceRef, NSUInteger) NS_RETURNS_RETAINED;
static id xcmDev_newTextureIOSurfaceSlice(id self, SEL, MTLTextureDescriptor *, IOSurfaceRef, NSUInteger, NSUInteger) NS_RETURNS_RETAINED;
static id xcmDev_newDefaultLibrary(id self, SEL) NS_RETURNS_RETAINED;
static id xcmDev_newDefaultLibraryWithBundle(id self, SEL, NSBundle *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithSource(id self, SEL, NSString *, MTLCompileOptions *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithData(id self, SEL, dispatch_data_t, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithURL(id self, SEL, NSURL *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithFile(id self, SEL, NSString *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithStitched(id self, SEL, MTLStitchedLibraryDescriptor *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newLibraryWithStitchedArchive(id self, SEL, MTLStitchedLibraryDescriptor *, id, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newComputePSO(id self, SEL, id<MTLFunction>, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newComputePSOOpts(id self, SEL, id<MTLFunction>, MTLPipelineOption, MTLComputePipelineReflection **, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newComputePSODesc(id self, SEL, MTLComputePipelineDescriptor *, MTLPipelineOption, MTLComputePipelineReflection **, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newRenderPSO(id self, SEL, MTLRenderPipelineDescriptor *, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newRenderPSOOpts(id self, SEL, MTLRenderPipelineDescriptor *, MTLPipelineOption, MTLRenderPipelineReflection **, NSError **) NS_RETURNS_RETAINED;
static id xcmDev_newSampler(id self, SEL, MTLSamplerDescriptor *) NS_RETURNS_RETAINED;
static id xcmDev_newDepthStencil(id self, SEL, MTLDepthStencilDescriptor *) NS_RETURNS_RETAINED;
static id xcmDev_newEvent(id self, SEL) NS_RETURNS_RETAINED;
static id xcmDev_newSharedEvent(id self, SEL) NS_RETURNS_RETAINED;
static id xcmDev_newSharedEventOpts(id self, SEL, NSUInteger) NS_RETURNS_RETAINED;
static id xcmDev_newSharedEventWithHandle(id self, SEL, MTLSharedEventHandle *) NS_RETURNS_RETAINED;
static id xcmDev_newFence(id self, SEL) NS_RETURNS_RETAINED;
static id xcmDev_newHeap(id self, SEL, MTLHeapDescriptor *) NS_RETURNS_RETAINED;

static id xcmDev_newCommandQueue(id self, SEL) {
    if (!gXm2vDevice) return nil;
    Xm2vQueue *q = xm2v_queue_new(gXm2vDevice);
    return q ? [[XcmQueue alloc] initWithHandle:q device:self] : nil;
}
static id xcmDev_newCommandQueueMax(id self, SEL, NSUInteger) {
    return xcmDev_newCommandQueue(self, nullptr);
}
static id xcmDev_newBufferLen(id self, SEL, NSUInteger length, MTLResourceOptions opts) {
    if (!gXm2vDevice || length == 0) return nil;
    Xm2vBuffer *b = xm2v_buffer_new(gXm2vDevice, length, opts, nullptr);
    if (xcmTraceOn()) fprintf(stderr, "XCM newBufferLen(%lu, 0x%lx) -> %p\n", (unsigned long)length, (unsigned long)opts, (void *)b);
    return b ? [[XcmBuffer alloc] initWithHandle:b device:self options:opts] : nil;
}
static id xcmDev_newBufferBytes(id self, SEL, const void *pointer, NSUInteger length, MTLResourceOptions opts) {
    if (!gXm2vDevice || !pointer || length == 0) return nil;
    Xm2vBuffer *b = xm2v_buffer_new(gXm2vDevice, length, opts, pointer);
    return b ? [[XcmBuffer alloc] initWithHandle:b device:self options:opts] : nil;
}
static void xcmBufferDeallocCallback(void *ptr, uint64_t length, void *ctx) {
    if (!ctx) return;
    void (^block)(void *, NSUInteger) = (__bridge_transfer void (^)(void *, NSUInteger))ctx;
    block(ptr, (NSUInteger)length);
}
static NSDictionary *xcmDev_noCopyStats(id, SEL) {
    return @{
        @"aliased": @(gNoCopyAliased.load(std::memory_order_relaxed)),
        @"fallbackNoDeallocator": @(gNoCopyFallbackNoDeallocator.load(std::memory_order_relaxed)),
        @"fallbackUnaligned": @(gNoCopyFallbackUnaligned.load(std::memory_order_relaxed)),
        @"fallbackReadOnly": @(gNoCopyFallbackReadOnly.load(std::memory_order_relaxed)),
        @"fallbackRemapErr": @(gNoCopyFallbackRemapErr.load(std::memory_order_relaxed)),
        @"commitSyncCopies": @(gNoCopyCommitSyncCopies.load(std::memory_order_relaxed)),
        @"commitSyncBytes": @(gNoCopyCommitSyncBytes.load(std::memory_order_relaxed)),
        @"maxAliasedLen": @(gNoCopyMaxAliasedLen.load(std::memory_order_relaxed)),
        @"maxNoDeallocLen": @(gNoCopyMaxNoDeallocLen.load(std::memory_order_relaxed)),
    };
}
static id xcmDev_newBufferNoCopy(id self, SEL, void *pointer, NSUInteger length, MTLResourceOptions opts, void (^deallocator)(void *, NSUInteger)) {
    // Match real macOS Metal: NULL pointer or 0 length returns nil without invoking deallocator.
    if (!gXm2vDevice || !pointer || length == 0) return nil;
    static int sForceNoCopyFallback = -1;
    if (sForceNoCopyFallback < 0) {
        const char *e = getenv("XNU_CPU_METAL_FORCE_NOCOPY_FALLBACK");
        sForceNoCopyFallback = (e && e[0] && e[0] != '0') ? 1 : 0;
    }
    // Only import caller memory directly via VK_EXT_external_memory_host when the
    // caller supplies a deallocator (transferring exclusive ownership until dealloc).
    // With deallocator == nil, callers such as CoreAnimation can unmap `pointer`
    // while in-flight command buffers are still executing in lavapipe (causing
    // EXC_BAD_ACCESS in util_copy_box).
    void *ctx = (!sForceNoCopyFallback && deallocator) ? (__bridge_retained void *)[deallocator copy] : nullptr;
    Xm2vBuffer *b = (sForceNoCopyFallback || !deallocator) ? nullptr :
        xm2v_buffer_new_no_copy_dealloc(gXm2vDevice, pointer, length, opts,
                                        ctx ? xcmBufferDeallocCallback : nullptr, ctx);
    if (!b) {
        if (ctx) (void)(__bridge_transfer id)ctx;
        size_t pageSz = (size_t)vm_page_size;
        bool callerAligned = (pageSz > 0) &&
                             (((uintptr_t)pointer & (pageSz - 1)) == 0) &&
                             ((length & (pageSz - 1)) == 0);
        b = callerAligned
            ? xm2v_buffer_new_page_aligned(gXm2vDevice, length, opts, pointer)
            : xm2v_buffer_new(gXm2vDevice, length, opts, pointer);
        if (!b) return nil;
        XcmBuffer *buf = [[XcmBuffer alloc] initWithHandle:b device:self options:opts];
        if (!buf) return nil;
        buf->_fallbackPtr = pointer;
        buf->_fallbackLen = length;
        if (deallocator) buf->_fallbackDeallocator = [deallocator copy];

        // True zero-copy physical-page aliasing: only when caller supplies a deallocator
        // (transferring exclusive VA ownership until dealloc). With deallocator == nil,
        // callers such as CoreAnimation can unmap `pointer` concurrently with async
        // buffer release, which would cause VM_FLAGS_OVERWRITE in dealloc to hit
        // EXC_GUARD / GUARD_TYPE_VIRT_MEMORY / DEALLOC_GAP or clobber a reused VA.
        void *gpuPtr = xm2v_buffer_contents(b);
        bool aligned = callerAligned &&
                       (gpuPtr != nullptr) &&
                       (((uintptr_t)gpuPtr & (pageSz - 1)) == 0);
        if (!deallocator) {
            gNoCopyFallbackNoDeallocator.fetch_add(1, std::memory_order_relaxed);
            xcmAtomicMax(gNoCopyMaxNoDeallocLen, (uint64_t)length);
            if (aligned && !xcmRangeIsFullyWritable(pointer, length)) {
                buf->_fallbackReadOnly = YES;
                gNoCopyFallbackReadOnly.fetch_add(1, std::memory_order_relaxed);
            }
            xcmMaybeLogNoCopyStats();
        } else if (!aligned) {
            uint64_t n = gNoCopyFallbackUnaligned.fetch_add(1, std::memory_order_relaxed) + 1;
            if (n <= 5 || (n % 256) == 0) {
                os_log_info(OS_LOG_DEFAULT,
                            "XnuCpuMetal: NoCopy alias skipped (unaligned, count=%llu ptr=%p len=%lu gpu=%p page=%zu)",
                            (unsigned long long)n, pointer, (unsigned long)length, gpuPtr, pageSz);
            }
        } else if (!xcmRangeIsFullyWritable(pointer, length)) {
            buf->_fallbackReadOnly = YES;
            uint64_t n = gNoCopyFallbackReadOnly.fetch_add(1, std::memory_order_relaxed) + 1;
            if (n <= 5 || (n % 256) == 0) {
                os_log_info(OS_LOG_DEFAULT,
                            "XnuCpuMetal: NoCopy alias skipped (read-only mapping, count=%llu ptr=%p len=%lu)",
                            (unsigned long long)n, pointer, (unsigned long)length);
            }
        } else {
            mach_vm_address_t target = (mach_vm_address_t)pointer;
            vm_prot_t curProt = 0, maxProt = 0;
            kern_return_t kr = mach_vm_remap(
                mach_task_self(),
                &target,
                (mach_vm_size_t)length,
                0,
                VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE,
                mach_task_self(),
                (mach_vm_address_t)gpuPtr,
                FALSE,
                &curProt,
                &maxProt,
                VM_INHERIT_NONE);
            if (kr == KERN_SUCCESS && target == (mach_vm_address_t)pointer && (curProt & VM_PROT_WRITE)) {
                buf->_fallbackAliased = YES;
                xcmAtomicMax(gNoCopyMaxAliasedLen, (uint64_t)length);
                uint64_t n = gNoCopyAliased.fetch_add(1, std::memory_order_relaxed) + 1;
                if (n == 1) {
                    os_log_info(OS_LOG_DEFAULT,
                                "XnuCpuMetal: newBufferWithBytesNoCopy using mach_vm_remap physical-page aliasing (page=%zu)",
                                pageSz);
                }
                xcmMaybeLogNoCopyStats();
            } else {
                uint64_t n = gNoCopyFallbackRemapErr.fetch_add(1, std::memory_order_relaxed) + 1;
                if (n <= 10 || (n % 256) == 0) {
                    os_log_info(OS_LOG_DEFAULT,
                                "XnuCpuMetal: NoCopy mach_vm_remap failed (kr=%d curProt=0x%x count=%llu ptr=%p len=%lu gpu=%p), using bounded fallback",
                                kr, curProt, (unsigned long long)n, pointer, (unsigned long)length, gpuPtr);
                }
            }
        }
        return buf;
    }
    return [[XcmBuffer alloc] initWithHandle:b device:self options:opts];
}
static id xcmDev_newTexture(id self, SEL, MTLTextureDescriptor *desc) {
    if (!gXm2vDevice || !desc) return nil;
    Xm2vTextureDesc td = {};
    td.texture_type = (uint64_t)desc.textureType;
    td.pixel_format = (uint64_t)desc.pixelFormat;
    td.width = desc.width;
    td.height = desc.height;
    td.depth = desc.depth;
    td.mipmap_level_count = desc.mipmapLevelCount;
    td.sample_count = desc.sampleCount;
    td.array_length = desc.arrayLength;
    td.resource_options = (uint64_t)desc.resourceOptions;
    td.usage = (uint64_t)desc.usage;
    Xm2vTexture *t = xm2v_texture_new(gXm2vDevice, &td);
    if (!t) {
        const char *err = xm2v_last_error() ?: "?";
        static int sNewTexFailLogs = 0;
        if (sNewTexFailLogs < 20) {
            sNewTexFailLogs++;
            os_log_error(OS_LOG_DEFAULT,
                         "XnuCpuMetal: newTexture failed (fmt=%lu type=%lu %lux%lux%lu mip=%lu samp=%lu arr=%lu usage=0x%lx opts=0x%lx): %{public}s",
                         (unsigned long)desc.pixelFormat, (unsigned long)desc.textureType,
                         (unsigned long)desc.width, (unsigned long)desc.height, (unsigned long)desc.depth,
                         (unsigned long)desc.mipmapLevelCount, (unsigned long)desc.sampleCount,
                         (unsigned long)desc.arrayLength, (unsigned long)desc.usage,
                         (unsigned long)desc.resourceOptions, err);
        }
        fprintf(stderr, "XCM newTexture -> %p fmt=%lu type=%lu %lux%lux%lu mip=%lu usage=0x%lx opts=0x%lx err=%s\n",
                (void *)t, (unsigned long)desc.pixelFormat, (unsigned long)desc.textureType,
                (unsigned long)desc.width, (unsigned long)desc.height, (unsigned long)desc.depth,
                (unsigned long)desc.mipmapLevelCount, (unsigned long)desc.usage, (unsigned long)desc.resourceOptions, err);
    } else if (xcmTraceOn()) {
        fprintf(stderr, "XCM newTexture -> %p fmt=%lu type=%lu %lux%lux%lu mip=%lu usage=0x%lx opts=0x%lx err=ok\n",
                (void *)t, (unsigned long)desc.pixelFormat, (unsigned long)desc.textureType,
                (unsigned long)desc.width, (unsigned long)desc.height, (unsigned long)desc.depth,
                (unsigned long)desc.mipmapLevelCount, (unsigned long)desc.usage, (unsigned long)desc.resourceOptions);
    }
    return t ? [[XcmTexture alloc] initWithHandle:t device:self descriptor:desc] : nil;
}
static id xcmDev_newTextureIOSurface(id self, SEL, MTLTextureDescriptor *desc, IOSurfaceRef surface, NSUInteger plane) {
    if (xcmTraceOn()) fprintf(stderr, "XCM enter newTextureIOSurface desc=%p surf=%p plane=%lu\n", (__bridge void *)desc, surface, (unsigned long)plane);
    if (!gXm2vDevice || !desc) return nil;
    // Two-plane YCbCr IOSurface formats (YCBCR8_420_2P, YCBCR10_420_2P, etc.) are created over
    // a shared backing buffer via metal2vulkan-runtime's IoSurfacePlane + IoSurfaceYCbCr path.
    if (surface && xm2v_pixel_format_is_ycbcr((uint64_t)desc.pixelFormat)) {
        if (IOSurfaceLock(surface, kIOSurfaceLockReadOnly, nullptr) == kIOReturnSuccess) {
            void *rootBase = IOSurfaceGetBaseAddress(surface);
            size_t allocSz = IOSurfaceGetAllocSize(surface);
            size_t planeCount = IOSurfaceGetPlaneCount(surface);
            uint32_t surfFmt = IOSurfaceGetPixelFormat(surface);
            uint64_t surfW = IOSurfaceGetWidth(surface);
            uint64_t surfH = IOSurfaceGetHeight(surface);
            bool planar = (planeCount > 1);
            void *lumaBase = (planeCount > 0 && plane < planeCount) ? IOSurfaceGetBaseAddressOfPlane(surface, plane) : rootBase;
            uint64_t lumaBpr = (planeCount > 0 && plane < planeCount) ? IOSurfaceGetBytesPerRowOfPlane(surface, plane) : IOSurfaceGetBytesPerRow(surface);
            uint64_t lumaOff = (rootBase && lumaBase && (uintptr_t)lumaBase >= (uintptr_t)rootBase)
                ? (uint64_t)((uint8_t *)lumaBase - (uint8_t *)rootBase) : 0;
            uint64_t chromaOff = 0, chromaBpr = 0;
            if (planeCount > 1) {
                void *chromaBase = IOSurfaceGetBaseAddressOfPlane(surface, 1);
                chromaBpr = IOSurfaceGetBytesPerRowOfPlane(surface, 1);
                if (rootBase && chromaBase && (uintptr_t)chromaBase >= (uintptr_t)rootBase) {
                    chromaOff = (uint64_t)((uint8_t *)chromaBase - (uint8_t *)rootBase);
                }
            }
            char matBuf[64] = {};
            CFTypeRef matRef = IOSurfaceCopyValue(surface, CFSTR("IOSurfaceYCbCrMatrix"));
            if (matRef) {
                if (CFGetTypeID(matRef) == CFStringGetTypeID()) {
                    CFStringGetCString((CFStringRef)matRef, matBuf, sizeof(matBuf), kCFStringEncodingUTF8);
                }
                CFRelease(matRef);
            }
            uint32_t seed = IOSurfaceGetSeed(surface);
            Xm2vBuffer *bh = (rootBase && allocSz > 0)
                ? xm2v_buffer_new(gXm2vDevice, allocSz, MTLResourceStorageModeShared, rootBase)
                : nullptr;
            IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, nullptr);
            if (bh) {
                XcmBuffer *backingBuf = [[XcmBuffer alloc] initWithHandle:bh device:self options:MTLResourceStorageModeShared];
                MTLTextureDescriptor *copy = [desc copy];
                copy.storageMode = MTLStorageModeShared;
                Xm2vTextureDesc td;
                xcmFillTextureDesc(copy, td);
                Xm2vTexture *t = xm2v_buffer_new_texture_with_iosurface_ycbcr(
                    bh, &td, lumaOff, lumaBpr, planar, surfFmt, plane, surfW, surfH,
                    chromaOff, chromaBpr, matBuf[0] ? matBuf : nullptr);
                if (t) {
                    XcmTexture *tex = [[XcmTexture alloc] initWithHandle:t device:self descriptor:copy];
                    if (tex) {
                        CFRetain(surface);
                        tex->_buffer = backingBuf;
                        tex->_iosurface = surface;
                        tex->_iosurfacePlane = plane;
                        tex->_iosurfaceSeed = seed;
                        tex->_iosurfaceSyncedIn = YES;
                        tex->_iosurfaceIsYCbCrBacking = YES;
                    }
                    return tex;
                } else {
                    static int sYcbcrFailLogs = 0;
                    if (sYcbcrFailLogs < 10) {
                        sYcbcrFailLogs++;
                        os_log_error(OS_LOG_DEFAULT,
                                     "XnuCpuMetal: newTextureIOSurface YCbCr failed (sid=0x%x surfFmt=0x%x plane=%lu descFmt=%lu %lux%lu luma=(%llu,%llu) chroma=(%llu,%llu) mat=%{public}s): %{public}s",
                                     IOSurfaceGetID(surface), surfFmt, (unsigned long)plane,
                                     (unsigned long)desc.pixelFormat, (unsigned long)desc.width, (unsigned long)desc.height,
                                     (unsigned long long)lumaOff, (unsigned long long)lumaBpr,
                                     (unsigned long long)chromaOff, (unsigned long long)chromaBpr,
                                     matBuf[0] ? matBuf : "(none)", xm2v_last_error() ?: "?");
                    }
                }
            }
        }
    }
    // Force Shared storage internally so replaceRegion/getBytes sync with IOSurface directly.
    MTLTextureDescriptor *copy = [desc copy];
    copy.storageMode = MTLStorageModeShared;
    XcmTexture *tex = xcmDev_newTexture(self, nullptr, copy);
    if (!tex) {
        static int sIosFailLogs = 0;
        if (sIosFailLogs < 20) {
            sIosFailLogs++;
            uint32_t sid = surface ? IOSurfaceGetID(surface) : 0;
            uint32_t pf = surface ? IOSurfaceGetPixelFormat(surface) : 0;
            os_log_error(OS_LOG_DEFAULT,
                         "XnuCpuMetal: newTextureIOSurface failed (sid=0x%x surfFmt=0x%x plane=%lu descFmt=%lu %lux%lu usage=0x%lx origOpts=0x%lx)",
                         sid, pf, (unsigned long)plane, (unsigned long)desc.pixelFormat,
                         (unsigned long)desc.width, (unsigned long)desc.height,
                         (unsigned long)desc.usage, (unsigned long)desc.resourceOptions);
        }
    }
    if (xcmTraceOn()) fprintf(stderr, "XCM newTextureIOSurface -> %p surf=%p plane=%lu\n", (__bridge void *)tex, surface, (unsigned long)plane);
    if (tex && surface) {
        CFRetain(surface);
        tex->_iosurface = surface;
        tex->_iosurfacePlane = plane;
        [tex syncFromIOSurfaceIfNeeded];
    }
    return tex;
}
static id xcmDev_newTextureIOSurfaceSlice(id self, SEL, MTLTextureDescriptor *desc, IOSurfaceRef surface, NSUInteger plane, NSUInteger slice) {
    (void)slice;
    return xcmDev_newTextureIOSurface(self, nullptr, desc, surface, plane);
}
static id xcmDev_newLibraryFromNSData(id self, NSData *nsdata, const char *origin, NSError **error) {
    if (!gXm2vDevice || !nsdata || nsdata.length == 0) {
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: newLibrary empty (origin=%{public}s data=%p len=%lu)",
                     origin ?: "?", (__bridge void *)nsdata, (unsigned long)nsdata.length);
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "empty library data");
        return nil;
    }
    Xm2vLibrary *lib = xm2v_library_new_with_data(gXm2vDevice, nsdata.bytes, nsdata.length);
    if (!lib) {
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: xm2v_library_new_with_data failed (origin=%{public}s len=%lu): %{public}s",
                     origin ?: "?", (unsigned long)nsdata.length, xm2v_last_error() ?: "?");
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Library load failed");
        return nil;
    }
    return [[XcmLibrary alloc] initWithHandle:lib device:self data:nsdata];
}
static id xcmDev_newLibraryWithData(id self, SEL, dispatch_data_t data, NSError **error) {
    if (!gXm2vDevice || !data) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Null library data");
        return nil;
    }
    const void *buf = nullptr;
    size_t len = 0;
    dispatch_data_t contig = dispatch_data_create_map(data, &buf, &len);
    NSData *nsdata = nil;
    if (contig && buf && len > 0) {
        // dispatch_data_t is toll-free bridged to NSData; avoid copying 187 MB default.metallib into dirty heap.
        nsdata = (NSData *)contig;
    } else {
        size_t total = dispatch_data_get_size(data);
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: newLibraryWithData map returned empty (data=%p cls=%{public}s total=%zu buf=%p len=%zu)",
                     (__bridge void *)data, object_getClassName((id)data), total, buf, len);
        if (total > 0) {
            NSMutableData *flat = [NSMutableData dataWithCapacity:total];
            if (flat) {
                dispatch_data_apply(data, ^bool(dispatch_data_t, size_t, const void *ptr, size_t sz) {
                    [flat appendBytes:ptr length:sz];
                    return true;
                });
                nsdata = flat;
            }
        }
    }
    return xcmDev_newLibraryFromNSData(self, nsdata, "dispatch_data", error);
}
static id xcmDev_newLibraryWithURL(id self, SEL, NSURL *url, NSError **error) {
    if (!url) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFileNotFound, "Nil library URL");
        return nil;
    }
    NSData *nsdata = [NSData dataWithContentsOfURL:url options:NSDataReadingMappedIfSafe error:error];
    if (!nsdata) {
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: newLibraryWithURL read failed (url=%{public}s): %{public}s",
                     url.path.UTF8String ?: "?", (error && *error) ? (*error).localizedDescription.UTF8String : "?");
        return nil;
    }
    return xcmDev_newLibraryFromNSData(self, nsdata, url.path.UTF8String ?: "url", error);
}
static id xcmDev_newLibraryWithFile(id self, SEL, NSString *filepath, NSError **error) {
    return xcmDev_newLibraryWithURL(self, nullptr, [NSURL fileURLWithPath:filepath], error);
}
static NSString *xcmQuoteArg(NSString *text) {
    if ([text rangeOfCharacterFromSet:[NSCharacterSet characterSetWithCharactersInString:@" \t\"\\'"]].location == NSNotFound)
        return text;
    NSString *escaped = [[text stringByReplacingOccurrencesOfString:@"\\" withString:@"\\\\"]
        stringByReplacingOccurrencesOfString:@"\"" withString:@"\\\""];
    return [NSString stringWithFormat:@"\"%@\"", escaped];
}

static NSString *xcmCompileArguments(MTLCompileOptions *options) {
    NSMutableArray<NSString *> *args = [NSMutableArray arrayWithObjects:@"--driver-mode=metal", @"-x", @"metal", nil];
    NSUInteger version = (options && options.languageVersion) ? (NSUInteger)options.languageVersion : (NSUInteger)MTLLanguageVersion3_1;
    unsigned major = (unsigned)(version >> 16), minor = (unsigned)(version & 0xffff);
    [args addObject:[NSString stringWithFormat:(major >= 3 ? @"-std=metal%u.%u" : @"-std=macos-metal%u.%u"), major, minor]];
    SEL modeSel = sel_registerName("mathMode"), fnSel = sel_registerName("mathFloatingPointFunctions");
    NSString *modeName = nil, *fnName = nil;
    if (options && [options respondsToSelector:modeSel] && [options respondsToSelector:fnSel]) {
        switch (((NSInteger (*)(id, SEL))objc_msgSend)(options, modeSel)) {
            case MTLMathModeSafe: modeName = @"safe"; break;
            case MTLMathModeRelaxed: modeName = @"relaxed"; break;
            case MTLMathModeFast: modeName = @"fast"; break;
        }
        switch (((NSInteger (*)(id, SEL))objc_msgSend)(options, fnSel)) {
            case MTLMathFloatingPointFunctionsFast: fnName = @"fast"; break;
            case MTLMathFloatingPointFunctionsPrecise: fnName = @"precise"; break;
        }
    }
    if (modeName && fnName) {
        [args addObject:[@"-fmetal-math-mode=" stringByAppendingString:modeName]];
        [args addObject:[@"-fmetal-math-fp32-functions=" stringByAppendingString:fnName]];
    } else {
        BOOL fast = options ? ((BOOL (*)(id, SEL))objc_msgSend)(options, sel_registerName("fastMathEnabled")) : YES;
        [args addObject:fast ? @"-ffast-math" : @"-fno-fast-math"];
    }
    if (options.preserveInvariance) [args addObject:@"-fpreserve-invariance"];
    if (options.optimizationLevel == MTLLibraryOptimizationLevelSize) [args addObject:@"-Os"];
    [options.preprocessorMacros enumerateKeysAndObjectsUsingBlock:^(NSString *name, NSObject *value, BOOL *) {
        [args addObject:xcmQuoteArg([NSString stringWithFormat:@"-D%@=%@", name, value])];
    }];
    [args addObject:xcmQuoteArg([@"-fmodules-cache-path=" stringByAppendingString:[NSTemporaryDirectory() stringByAppendingPathComponent:@"xcm-msl"]])];
    return [args componentsJoinedByString:@" "];
}

static id xcmDev_newDefaultLibraryWithBundle(id self, SEL, NSBundle *bundle, NSError **error) {
    NSURL *url = [bundle URLForResource:@"default" withExtension:@"metallib"];
    if (!url) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorFileNotFound, "Bundle has no default.metallib");
        return nil;
    }
    return xcmDev_newLibraryWithURL(self, nullptr, url, error);
}
static id xcmDev_newDefaultLibrary(id self, SEL) {
    return xcmDev_newDefaultLibraryWithBundle(self, nullptr, [NSBundle mainBundle], nil);
}
static id xcmDev_newLibraryWithSource(id self, SEL, NSString *source, MTLCompileOptions *options, NSError **error) {
    if (!source) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Nil source");
        return nil;
    }
    NSData *text = [source dataUsingEncoding:NSUTF8StringEncoding];
    NSString *argStr = xcmCompileArguments(options);
    const char *argC = argStr.UTF8String ?: "";
    uint64_t sourceBytes = (text.length + 1 + 3) & ~uint64_t(3);
    uint64_t optionBytes = strlen(argC) + 1;
    std::vector<uint8_t> req(16 + sourceBytes + optionBytes, 0);
    memcpy(req.data(), &sourceBytes, 8);
    memcpy(req.data() + 8, &optionBytes, 8);
    memcpy(req.data() + 16, text.bytes, text.length);
    memcpy(req.data() + 16 + sourceBytes, argC, optionBytes);
    NSString *warnings = nil, *failure = nil;
    NSData *metallib = xcmCodeGenRequest(13, req, &warnings, &failure);
    if (!metallib) {
        if (xcmTraceOn()) fprintf(stderr, "XCM newLibraryWithSource failed: %s\n", failure.UTF8String ?: "?");
        if (error) *error = [NSError errorWithDomain:MTLLibraryErrorDomain code:MTLLibraryErrorCompileFailure
                                            userInfo:@{NSLocalizedDescriptionKey: failure ?: @"Metal source compilation failed"}];
        return nil;
    }
    id<MTLLibrary> lib = xcmDev_newLibraryFromNSData(self, metallib, "source", error);
    if (lib && warnings.length && error)
        *error = [NSError errorWithDomain:MTLLibraryErrorDomain code:MTLLibraryErrorCompileWarning
                                 userInfo:@{NSLocalizedDescriptionKey: warnings}];
    return lib;
}
static void xcmDev_newLibraryWithSourceAsync(id self, SEL, NSString *source, MTLCompileOptions *options, MTLNewLibraryCompletionHandler handler) {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSError *err = nil;
        id<MTLLibrary> lib = xcmDev_newLibraryWithSource(self, nullptr, source, options, &err);
        if (handler) handler(lib, err);
    });
}
static id xcmDev_newLibraryWithStitched(id self, SEL, MTLStitchedLibraryDescriptor *d, NSError **error) {
    NSArray<MTLFunctionStitchingGraph *> *graphs = d.functionGraphs;
    NSArray<id<MTLFunction>> *functions = d.functions;
    if (!graphs.count || !functions.count) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Stitched library needs graphs and functions");
        return nil;
    }
    NSString *dag = [(id<XcmOriginalStitching>)self newDagStringWithGraphs:graphs];
    if (!dag.length) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Empty stitching DAG text");
        return nil;
    }
    std::vector<uint8_t> req;
    auto pushWord = [&](uint32_t v) {
        const uint8_t *b = (const uint8_t *)&v;
        req.insert(req.end(), b, b + 4);
    };
    const char *text = dag.UTF8String;
    pushWord(0x64616720); // 'dag '
    req.insert(req.end(), text, text + strlen(text) + 1);
    pushWord(0x6e756d66); // 'numf'
    pushWord((uint32_t)functions.count);
    for (id fn in functions) {
        NSData *bc = [fn isKindOfClass:[XcmFunction class]] ? xcmExtractFunctionBitcode((XcmFunction *)fn) : nil;
        if (!bc) {
            if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Missing bitcode for stitched function");
            return nil;
        }
        pushWord(0x62697463); // 'bitc'
        pushWord((uint32_t)bc.length);
        const uint8_t *b = (const uint8_t *)bc.bytes;
        req.insert(req.end(), b, b + bc.length);
    }
    // macOS 27 MTLCompilerObject::stitchFunctionDag trailing slots
    pushWord(0x6e756d73); pushWord(0); // 'nums'
    pushWord(0x7074666d); pushWord(0); // 'ptfm'
    pushWord(0x61726964); pushWord(0); // 'arid'

    NSString *warnings = nil, *failure = nil;
    NSData *metallib = xcmCodeGenRequest(14, req, &warnings, &failure);
    if (!metallib) {
        if (error) *error = [NSError errorWithDomain:MTLLibraryErrorDomain
                                                code:MTLLibraryErrorCompileFailure
                                            userInfo:@{NSLocalizedDescriptionKey: failure ?: @"Stitched library compilation failed"}];
        return nil;
    }
    return xcmDev_newLibraryFromNSData(self, metallib, "stitched", error);
}
static id xcmDev_newLibraryWithStitchedArchive(id self, SEL, MTLStitchedLibraryDescriptor *d, id, NSError **error) {
    return xcmDev_newLibraryWithStitched(self, nullptr, d, error);
}
static void xcmDev_newLibraryWithStitchedAsync(id self, SEL, MTLStitchedLibraryDescriptor *d, MTLNewLibraryCompletionHandler handler) {
    NSError *err = nil;
    id<MTLLibrary> lib = xcmDev_newLibraryWithStitched(self, nullptr, d, &err);
    if (handler) handler(lib, err);
}
static id xcmDev_newComputePSO(id self, SEL, id<MTLFunction> fn, NSError **error) {
    if (!gXm2vDevice || ![fn isKindOfClass:[XcmFunction class]] || !((XcmFunction *)fn)->_handle) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Invalid function");
        return nil;
    }
    Xm2vComputePipeline *pso = xm2v_compute_pipeline_new(gXm2vDevice, ((XcmFunction *)fn)->_handle);
    if (!pso) {
        static int sCpsoFailLogs = 0;
        if (sCpsoFailLogs < 20) {
            sCpsoFailLogs++;
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: newComputePSO failed (fn=%{public}s): %{public}s",
                         fn.name.UTF8String ?: "?", xm2v_last_error() ?: "?");
        }
    }
    if (xcmTraceOn() || !pso) {
        fprintf(stderr, "XCM newComputePSO(%s) -> %p err=%s\n",
                fn.name.UTF8String ?: "?", (void *)pso, pso ? "ok" : xm2v_last_error());
    }
    if (!pso) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Compute PSO failed");
        return nil;
    }
    return [[XcmComputePipeline alloc] initWithHandle:pso device:self];
}
static id xcmDev_newComputePSOOpts(id self, SEL, id<MTLFunction> fn, MTLPipelineOption, MTLComputePipelineReflection **refl, NSError **error) {
    if (refl) *refl = nil;
    return xcmDev_newComputePSO(self, nullptr, fn, error);
}
static void xcmCollectLinkedFunctions(MTLLinkedFunctions *linked,
                                      std::vector<Xm2vFunction *> &pubOut,
                                      std::vector<Xm2vFunction *> &privOut) {
    if (!linked) return;
    auto push = [](NSArray<id<MTLFunction>> *arr, std::vector<Xm2vFunction *> &dst) {
        for (id fn in arr) {
            if ([fn isKindOfClass:[XcmFunction class]] && ((XcmFunction *)fn)->_handle)
                dst.push_back(((XcmFunction *)fn)->_handle);
        }
    };
    push(linked.functions, pubOut);
    push(linked.binaryFunctions, pubOut);
    push(linked.privateFunctions, privOut);
}

static id xcmDev_newComputePSODesc(id self, SEL, MTLComputePipelineDescriptor *desc, MTLPipelineOption opts, MTLComputePipelineReflection **refl, NSError **error) {
    (void)opts;
    if (refl) *refl = nil;
    if (!gXm2vDevice || !desc || ![desc.computeFunction isKindOfClass:[XcmFunction class]] || !((XcmFunction *)desc.computeFunction)->_handle) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Invalid compute pipeline descriptor");
        return nil;
    }
    std::vector<Xm2vFunction *> pubLinked, privLinked;
    xcmCollectLinkedFunctions(desc.linkedFunctions, pubLinked, privLinked);
    Xm2vComputePipeline *pso = (pubLinked.empty() && privLinked.empty())
        ? xm2v_compute_pipeline_new(gXm2vDevice, ((XcmFunction *)desc.computeFunction)->_handle)
        : xm2v_compute_pipeline_new_with_linked(
              gXm2vDevice, ((XcmFunction *)desc.computeFunction)->_handle,
              pubLinked.empty() ? nullptr : pubLinked.data(), (uint32_t)pubLinked.size(),
              privLinked.empty() ? nullptr : privLinked.data(), (uint32_t)privLinked.size());
    if (!pso) {
        static int sCpsoDescFailLogs = 0;
        if (sCpsoDescFailLogs < 20) {
            sCpsoDescFailLogs++;
            os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: newComputePSODesc failed (fn=%{public}s): %{public}s",
                         desc.computeFunction.name.UTF8String ?: "?", xm2v_last_error() ?: "?");
        }
    }
    if (xcmTraceOn() || !pso) {
        fprintf(stderr, "XCM newComputePSODesc(%s, linked=%zu+%zu) -> %p err=%s\n",
                desc.computeFunction.name.UTF8String ?: "?", pubLinked.size(), privLinked.size(),
                (void *)pso, pso ? "ok" : xm2v_last_error());
    }
    if (!pso) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Compute PSO failed");
        return nil;
    }
    return [[XcmComputePipeline alloc] initWithHandle:pso device:self];
}
static void xcmDev_newComputePSOAsync(id self, SEL, id<MTLFunction> fn, void (^handler)(id<MTLComputePipelineState>, NSError *)) {
    NSError *err = nil;
    id pso = xcmDev_newComputePSO(self, nullptr, fn, &err);
    if (handler) handler(pso, err);
}
static id xcmDev_newRenderPSO(id self, SEL, MTLRenderPipelineDescriptor *desc, NSError **error) {
    if (!gXm2vDevice || !desc || ![desc.vertexFunction isKindOfClass:[XcmFunction class]] || !((XcmFunction *)desc.vertexFunction)->_handle) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorInternal, "Invalid render pipeline descriptor");
        return nil;
    }
    Xm2vRenderPipelineDesc rd = {};
    rd.vertex_function = ((XcmFunction *)desc.vertexFunction)->_handle;
    if (desc.fragmentFunction && [desc.fragmentFunction isKindOfClass:[XcmFunction class]])
        rd.fragment_function = ((XcmFunction *)desc.fragmentFunction)->_handle;
    std::vector<Xm2vFunction *> vPub, vPriv, fPub, fPriv;
    xcmCollectLinkedFunctions(desc.vertexLinkedFunctions, vPub, vPriv);
    xcmCollectLinkedFunctions(desc.fragmentLinkedFunctions, fPub, fPriv);
    rd.vertex_linked_functions = vPub.empty() ? nullptr : vPub.data();
    rd.vertex_linked_function_count = (uint32_t)vPub.size();
    rd.vertex_private_linked_functions = vPriv.empty() ? nullptr : vPriv.data();
    rd.vertex_private_linked_function_count = (uint32_t)vPriv.size();
    rd.fragment_linked_functions = fPub.empty() ? nullptr : fPub.data();
    rd.fragment_linked_function_count = (uint32_t)fPub.size();
    rd.fragment_private_linked_functions = fPriv.empty() ? nullptr : fPriv.data();
    rd.fragment_private_linked_function_count = (uint32_t)fPriv.size();
    rd.color_attachment_count = 8;
    for (int i = 0; i < 8; i++) {
        MTLRenderPipelineColorAttachmentDescriptor *ca = desc.colorAttachments[i];
        if (ca && ca.pixelFormat != MTLPixelFormatInvalid) {
            rd.color_attachments[i].pixel_format = (uint64_t)ca.pixelFormat;
            rd.color_attachments[i].blending_enabled = ca.isBlendingEnabled;
            rd.color_attachments[i].rgb_blend_op = (uint64_t)ca.rgbBlendOperation;
            rd.color_attachments[i].alpha_blend_op = (uint64_t)ca.alphaBlendOperation;
            rd.color_attachments[i].src_rgb_blend = (uint64_t)ca.sourceRGBBlendFactor;
            rd.color_attachments[i].dst_rgb_blend = (uint64_t)ca.destinationRGBBlendFactor;
            rd.color_attachments[i].src_alpha_blend = (uint64_t)ca.sourceAlphaBlendFactor;
            rd.color_attachments[i].dst_alpha_blend = (uint64_t)ca.destinationAlphaBlendFactor;
            rd.color_attachments[i].write_mask = (uint64_t)ca.writeMask;
        }
    }
    rd.depth_attachment_pixel_format = (uint64_t)desc.depthAttachmentPixelFormat;
    rd.stencil_attachment_pixel_format = (uint64_t)desc.stencilAttachmentPixelFormat;
    rd.sample_count = desc.rasterSampleCount;
    std::vector<Xm2vVertexAttribute> vattrs;
    std::vector<Xm2vVertexLayout> vlayouts;
    if (MTLVertexDescriptor *vd = desc.vertexDescriptor) {
        for (NSUInteger i = 0; i < 31; i++) {
            MTLVertexAttributeDescriptor *a = vd.attributes[i];
            if (a && a.format != MTLVertexFormatInvalid) {
                Xm2vVertexAttribute xa = {};
                xa.index = (uint32_t)i;
                xa.format = (uint64_t)a.format;
                xa.offset = (uint32_t)a.offset;
                xa.buffer_index = (uint32_t)a.bufferIndex;
                vattrs.push_back(xa);
            }
            MTLVertexBufferLayoutDescriptor *l = vd.layouts[i];
            if (l && (l.stride != 0 || l.stride == MTLBufferLayoutStrideDynamic)) {
                Xm2vVertexLayout xl = {};
                xl.index = (uint32_t)i;
                xl.dynamic_stride = (l.stride == MTLBufferLayoutStrideDynamic);
                xl.stride = xl.dynamic_stride ? 0 : (uint64_t)l.stride;
                xl.step_function = (uint64_t)l.stepFunction;
                xl.step_rate = (uint32_t)l.stepRate;
                vlayouts.push_back(xl);
            }
        }
    }
    rd.vertex_attributes = vattrs.empty() ? nullptr : vattrs.data();
    rd.vertex_attribute_count = (uint32_t)vattrs.size();
    rd.vertex_layouts = vlayouts.empty() ? nullptr : vlayouts.data();
    rd.vertex_layout_count = (uint32_t)vlayouts.size();
    Xm2vRenderPipeline *pso = xm2v_render_pipeline_new(gXm2vDevice, &rd);
    if (!pso) {
        // Retry once with unspecified declared function constants defaulted to zero
        // (QuartzCore/RenderBox omit default-false constants that gate stage_in attributes / color inputs).
        auto rebuildZeroRest = [](id fnObj) -> Xm2vFunction * {
            if (!fnObj || ![fnObj isKindOfClass:[XcmFunction class]]) return nullptr;
            XcmFunction *xf = (XcmFunction *)fnObj;
            if (!xf->_library || !xf->_library->_handle) return nullptr;
            NSString *srcName = xf->_sourceName ?: xf->_name;
            if (!srcName) return nullptr;
            std::vector<Xm2vConstant> constants;
            std::deque<std::string> nameStorage;
            std::deque<std::vector<uint8_t>> byteStorage;
            xcmExtractConstants(xf->_constantValues, constants, nameStorage, byteStorage);
            return xm2v_library_new_function_specialized_zero_rest(
                xf->_library->_handle, srcName.UTF8String,
                constants.empty() ? nullptr : constants.data(), (uint32_t)constants.size());
        };
        Xm2vFunction *zv = rebuildZeroRest(desc.vertexFunction);
        Xm2vFunction *zf = desc.fragmentFunction ? rebuildZeroRest(desc.fragmentFunction) : nullptr;
        if (zv || zf) {
            Xm2vRenderPipelineDesc rd2 = rd;
            if (zv) rd2.vertex_function = zv;
            if (zf) rd2.fragment_function = zf;
            pso = xm2v_render_pipeline_new(gXm2vDevice, &rd2);
            if (xcmTraceOn()) {
                fprintf(stderr, "XCM newRenderPSO zeroRest retry(v=%s,f=%s) -> %p err=%s\n",
                        desc.vertexFunction.name.UTF8String ?: "?",
                        desc.fragmentFunction.name.UTF8String ?: "-",
                        (void *)pso, pso ? "ok" : xm2v_last_error());
            }
            if (zv) xm2v_function_release(zv);
            if (zf) xm2v_function_release(zf);
        }
    }
    // If Metal's vertex/fragment shader declares a stage_in attribute or color input attachment
    // that the descriptor omitted (e.g. QuartzCore Vfx unused attribute 4, or sparse RenderBox
    // color attachments), fill the missing slot with a safe default and retry.
    for (int attempt = 0; !pso && attempt < 8; attempt++) {
        const char *err = xm2v_last_error();
        if (!err) break;
        unsigned missingIdx = 0;
        if (const char *va = strstr(err, "InvalidDescriptor(VertexAttribute { attribute: ")) {
            if (sscanf(va, "InvalidDescriptor(VertexAttribute { attribute: %u", &missingIdx) == 1 && missingIdx < 31) {
                bool exists = false;
                for (const auto &a : vattrs) if (a.index == missingIdx) { exists = true; break; }
                if (exists) break;
                uint32_t bufIdx = vlayouts.empty() ? 0 : vlayouts[0].index;
                if (vlayouts.empty()) {
                    Xm2vVertexLayout xl = {};
                    xl.index = 0; xl.stride = 16; xl.step_function = (uint64_t)MTLVertexStepFunctionConstant; xl.step_rate = 0;
                    vlayouts.push_back(xl);
                }
                Xm2vVertexAttribute xa = {};
                xa.index = missingIdx;
                xa.format = (uint64_t)MTLVertexFormatFloat4;
                xa.offset = 0;
                xa.buffer_index = bufIdx;
                vattrs.push_back(xa);
                rd.vertex_attributes = vattrs.data();
                rd.vertex_attribute_count = (uint32_t)vattrs.size();
                rd.vertex_layouts = vlayouts.data();
                rd.vertex_layout_count = (uint32_t)vlayouts.size();
                pso = xm2v_render_pipeline_new(gXm2vDevice, &rd);
                if (xcmTraceOn()) fprintf(stderr, "XCM newRenderPSO missing-vattr(%u) retry -> %p err=%s\n", missingIdx, (void *)pso, pso ? "ok" : xm2v_last_error());
                continue;
            }
        }
        if (const char *ca = strstr(err, "InvalidDescriptor(ColorAttachment { index: ")) {
            if (sscanf(ca, "InvalidDescriptor(ColorAttachment { index: %u", &missingIdx) == 1 && missingIdx < 8) {
                if (rd.color_attachments[missingIdx].pixel_format != 0) break;
                uint64_t fallbackFmt = rd.color_attachments[0].pixel_format ? rd.color_attachments[0].pixel_format : (uint64_t)MTLPixelFormatRGBA16Float;
                rd.color_attachments[missingIdx].pixel_format = fallbackFmt;
                rd.color_attachments[missingIdx].write_mask = (uint64_t)MTLColorWriteMaskAll;
                pso = xm2v_render_pipeline_new(gXm2vDevice, &rd);
                if (xcmTraceOn()) fprintf(stderr, "XCM newRenderPSO missing-color(%u) retry -> %p err=%s\n", missingIdx, (void *)pso, pso ? "ok" : xm2v_last_error());
                continue;
            }
        }
        break;
    }
    if (!pso) {
        static int sPsoFailLogs = 0;
        if (sPsoFailLogs < 20) {
            sPsoFailLogs++;
            os_log_error(OS_LOG_DEFAULT,
                         "XnuCpuMetal: newRenderPSO failed (v=%{public}s f=%{public}s fmt0=%llu): %{public}s",
                         desc.vertexFunction.name.UTF8String ?: "?",
                         desc.fragmentFunction.name.UTF8String ?: "-",
                         (unsigned long long)rd.color_attachments[0].pixel_format,
                         xm2v_last_error() ?: "?");
        }
    }
    if (xcmTraceOn() || !pso) {
        const auto &c0 = rd.color_attachments[0];
        fprintf(stderr, "XCM newRenderPSO(v=%s,f=%s, fmt0=%llu blend=%d[op=%llu/%llu rgb=%llu,%llu a=%llu,%llu] mask=0x%llx, vl=%zu+%zu, fl=%zu+%zu) -> %p err=%s\n",
                desc.vertexFunction.name.UTF8String ?: "?",
                desc.fragmentFunction.name.UTF8String ?: "-",
                (unsigned long long)c0.pixel_format, (int)c0.blending_enabled,
                (unsigned long long)c0.rgb_blend_op, (unsigned long long)c0.alpha_blend_op,
                (unsigned long long)c0.src_rgb_blend, (unsigned long long)c0.dst_rgb_blend,
                (unsigned long long)c0.src_alpha_blend, (unsigned long long)c0.dst_alpha_blend,
                (unsigned long long)c0.write_mask,
                vPub.size(), vPriv.size(), fPub.size(), fPriv.size(),
                (void *)pso, pso ? "ok" : xm2v_last_error());
        if (!pso && !vattrs.empty()) {
            fprintf(stderr, "  vattrs(%zu):", vattrs.size());
            for (const auto &a : vattrs) fprintf(stderr, " [idx=%u fmt=%llu off=%u buf=%u]", a.index, a.format, a.offset, a.buffer_index);
            fprintf(stderr, "\n  vlayouts(%zu):", vlayouts.size());
            for (const auto &l : vlayouts) fprintf(stderr, " [idx=%u stride=%llu dyn=%d step=%llu rate=%u]", l.index, l.stride, l.dynamic_stride, l.step_function, l.step_rate);
            fprintf(stderr, "\n");
        }
        if (!pso && desc.fragmentLinkedFunctions) {
            MTLLinkedFunctions *fl = desc.fragmentLinkedFunctions;
            fprintf(stderr, "  fl.functions=%lu binary=%lu private=%lu groups=%lu\n",
                    (unsigned long)fl.functions.count, (unsigned long)fl.binaryFunctions.count,
                    (unsigned long)fl.privateFunctions.count, (unsigned long)fl.groups.count);
            for (id fn in fl.functions)
                fprintf(stderr, "    fn: %s (%s)\n", object_getClassName(fn), [fn respondsToSelector:@selector(name)] ? [[fn name] UTF8String] : "?");
            for (id fn in fl.privateFunctions)
                fprintf(stderr, "    priv: %s (%s)\n", object_getClassName(fn), [fn respondsToSelector:@selector(name)] ? [[fn name] UTF8String] : "?");
            for (id fn in fl.binaryFunctions)
                fprintf(stderr, "    bin: %s (%s)\n", object_getClassName(fn), [fn respondsToSelector:@selector(name)] ? [[fn name] UTF8String] : "?");
        }
    }
    if (!pso) {
        if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorCompileFailure, "Render PSO failed");
        return nil;
    }
    XcmRenderPipeline *rp = [[XcmRenderPipeline alloc] initWithHandle:pso device:self];
    if (rp) {
        rp->_vName = [desc.vertexFunction.name copy];
        rp->_fName = [desc.fragmentFunction.name copy];
        rp->_blendEnabled = rd.color_attachments[0].blending_enabled ? YES : NO;
        rp->_writeMask = rd.color_attachments[0].write_mask;
    }
    return rp;
}
static id xcmDev_newRenderPSOOpts(id self, SEL, MTLRenderPipelineDescriptor *desc, MTLPipelineOption, MTLRenderPipelineReflection **refl, NSError **error) {
    if (refl) *refl = nil;
    return xcmDev_newRenderPSO(self, nullptr, desc, error);
}
static id xcmDev_newSampler(id self, SEL, MTLSamplerDescriptor *desc) {
    if (!gXm2vDevice || !desc) return nil;
    Xm2vSamplerDesc sd = {};
    sd.min_filter = (uint64_t)desc.minFilter;
    sd.mag_filter = (uint64_t)desc.magFilter;
    sd.mip_filter = (uint64_t)desc.mipFilter;
    sd.s_address_mode = (uint64_t)desc.sAddressMode;
    sd.t_address_mode = (uint64_t)desc.tAddressMode;
    sd.r_address_mode = (uint64_t)desc.rAddressMode;
    sd.max_anisotropy = desc.maxAnisotropy;
    sd.compare_function = (uint64_t)desc.compareFunction;
    sd.lod_min_clamp = desc.lodMinClamp;
    sd.lod_max_clamp = desc.lodMaxClamp;
    sd.normalized_coordinates = desc.normalizedCoordinates;
    Xm2vSampler *s = xm2v_sampler_new(gXm2vDevice, &sd);
    return s ? [[XcmSampler alloc] initWithHandle:s device:self] : nil;
}
static id xcmDev_newDepthStencil(id self, SEL, MTLDepthStencilDescriptor *desc) {
    if (!gXm2vDevice || !desc) return nil;
    Xm2vDepthStencilDesc dsd = {};
    dsd.depth_compare_function = (uint64_t)desc.depthCompareFunction;
    dsd.depth_write_enabled = desc.isDepthWriteEnabled;
    auto fillStencil = [](MTLStencilDescriptor *s, Xm2vStencilDesc &dst) {
        if (!s) return;
        dst.enabled = true;
        dst.stencil_compare_function = (uint64_t)s.stencilCompareFunction;
        dst.stencil_failure_operation = (uint64_t)s.stencilFailureOperation;
        dst.depth_failure_operation = (uint64_t)s.depthFailureOperation;
        dst.depth_stencil_pass_operation = (uint64_t)s.depthStencilPassOperation;
        dst.read_mask = s.readMask;
        dst.write_mask = s.writeMask;
    };
    fillStencil(desc.frontFaceStencil, dsd.front_face_stencil);
    fillStencil(desc.backFaceStencil, dsd.back_face_stencil);
    Xm2vDepthStencil *ds = xm2v_depth_stencil_new(gXm2vDevice, &dsd);
    return ds ? [[XcmDepthStencil alloc] initWithHandle:ds device:self] : nil;
}
static id xcmDev_newEvent(id self, SEL) {
    if (!gXm2vDevice) return nil;
    Xm2vEvent *ev = xm2v_event_new(gXm2vDevice);
    return ev ? [[XcmEvent alloc] initWithHandle:ev device:self] : nil;
}
static id xcmDev_newSharedEvent(id self, SEL) {
    (void)self;
    if (!gXm2vDevice) return nil;
    Xm2vSharedEvent *ev = xm2v_shared_event_new(gXm2vDevice);
    return ev ? [[XcmSharedEvent alloc] initWithHandle:ev] : nil;
}
static id xcmDev_newSharedEventOpts(id self, SEL, NSUInteger options) {
    (void)options;
    return xcmDev_newSharedEvent(self, nullptr);
}
static id xcmDev_newSharedEventWithHandle(id self, SEL, MTLSharedEventHandle *handle) {
    (void)self;
    if (!gXm2vDevice || !handle || ![(id)handle isKindOfClass:[XcmSharedEventHandle class]]) return nil;
    Xm2vSharedEvent *ev = xm2v_shared_event_new_with_handle(gXm2vDevice, ((XcmSharedEventHandle *)handle)->_handle);
    return ev ? [[XcmSharedEvent alloc] initWithHandle:ev] : nil;
}
static id xcmDev_newFence(id self, SEL) {
    if (!gXm2vDevice) return nil;
    Xm2vFence *f = xm2v_fence_new(gXm2vDevice);
    return f ? [[XcmFence alloc] initWithHandle:f device:self] : nil;
}
static id xcmDev_newHeap(id self, SEL, MTLHeapDescriptor *desc) {
    if (xcmTraceOn()) fprintf(stderr, "XCM newHeap(size=%lu storage=%lu)\n", (unsigned long)(desc ? desc.size : 0), (unsigned long)(desc ? desc.storageMode : 0));
    if (!desc) return nil;
    return [[XcmHeap alloc] initWithDevice:self descriptor:desc];
}
static MTLSizeAndAlign xcmDev_heapTexSizeAndAlign(id, SEL, MTLTextureDescriptor *desc) {
    NSUInteger w = desc ? MAX(1ul, desc.width) : 1;
    NSUInteger h = desc ? MAX(1ul, desc.height) : 1;
    NSUInteger d = desc ? MAX(1ul, desc.depth) : 1;
    NSUInteger sz = ((w * h * d * 16 + 255) & ~255ul);
    return MTLSizeAndAlign{sz, 256};
}
static MTLSizeAndAlign xcmDev_heapBufSizeAndAlign(id, SEL, NSUInteger len, MTLResourceOptions) { return MTLSizeAndAlign{len, 256}; }

// Metal's loader picks a slice of a binary archive / fat metallib by [device targetDeviceArchitecture] (cpuType, cpuSubtype);
// nil asserts in +[MTLLoader sliceIDForDevice:...]. This device runs AIR (through metal2vulkan), so it names the AIR type.
static id xcmDev_targetDeviceArchitecture(id, SEL) {
    static id arch;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        int type = 0x01000017, subtype = 0;
        if (const char *e = getenv("XNU_CPU_METAL_ARCH_TYPE")) type = (int)strtol(e, nullptr, 0);
        if (const char *e = getenv("XNU_CPU_METAL_ARCH_SUBTYPE")) subtype = (int)strtol(e, nullptr, 0);
        // Metal's internal MTLTargetDeviceArchitecture (cpuType, subType, driverVersion), not the public MTLArchitecture.
        Class c = objc_getClass("MTLTargetDeviceArchitecture");
        if (c) {
            arch = [[c alloc] init];
            ((void (*)(id, SEL, unsigned))objc_msgSend)(arch, sel_registerName("setCpuType:"), (unsigned)type);
            ((void (*)(id, SEL, unsigned))objc_msgSend)(arch, sel_registerName("setSubType:"), (unsigned)subtype);
            unsigned long long dv = 0;
            if (const char *e = getenv("XNU_CPU_METAL_DRIVER_VERSION")) dv = strtoull(e, nullptr, 0);
            ((void (*)(id, SEL, unsigned long long))objc_msgSend)(arch, sel_registerName("setDriverVersion:"), dv);
        }
        if (xcmTraceOn()) fprintf(stderr, "XCM targetDeviceArchitecture -> %p (0x%x/%d)\n", (__bridge void *)arch, type, subtype);
    });
    return arch;
}

static MTLArgumentBuffersTier xcmDev_argBufTier(id, SEL) { return MTLArgumentBuffersTier2; }
static BOOL xcmDev_no(id, SEL) { return NO; }
static void xcmDev_noop(id, SEL) {}
static void xcmDev_noopBool(id, SEL, BOOL) {}
static void xcmDev_noopReserve(id, SEL, NSUInteger, NSUInteger *, NSUInteger) {}
static NSUInteger xcmDev_zeroU(id, SEL) { return 0; }
static NSUInteger xcmDev_zeroU1(id, SEL, NSUInteger) { return 0; }
static NSUInteger xcmDev_singleFPConfig(id, SEL) { return 0x54; }
static NSUInteger xcmDev_halfFPConfig(id, SEL) { return 0xe; }
static NSUInteger xcmDev_maxViewportCount(id, SEL) { return 16; }
static NSUInteger xcmDev_maxArgBufSamplers(id, SEL) { return 16; }
static MTLSize xcmDev_sparseTileSize(id, SEL, MTLTextureType, MTLPixelFormat, NSUInteger) { return MTLSizeMake(0, 0, 0); }
static MTLSize xcmDev_sparseTileSizePage(id, SEL, MTLTextureType, MTLPixelFormat, NSUInteger, NSInteger) { return MTLSizeMake(0, 0, 0); }
static MTLSize xcmDev_tileSizeSparse(id, SEL, NSInteger, MTLTextureType, MTLPixelFormat, NSUInteger) { return MTLSizeMake(0, 0, 0); }
static id xcmDev_nil1(id, SEL, id) { return nil; }
static id xcmDev_nilPtr1(id, SEL, void *) { return nil; }
static id xcmDev_nil2(id, SEL, id, BOOL) { return nil; }
static id xcmDev_nilResGrp(id, SEL, const id *, NSUInteger) { return nil; }
static id xcmDev_nilICB(id, SEL, id, NSUInteger, MTLResourceOptions) { return nil; }
static id xcmDev_nilTexNoCopy(id, SEL, void *, NSUInteger, id, id) { return nil; }
static BOOL xcmDev_noMapSample(id, SEL, void *, NSUInteger, NSUInteger) { return NO; }
static id xcmDev_newTilePSO(id, SEL, id, NSError **error) {
    if (error) *error = xcmError(MTLLibraryErrorDomain, MTLLibraryErrorUnsupported, "Tile render pipelines not supported on CPU device");
    return nil;
}
static void xcmDev_newTilePSOAsync(id self, SEL, id desc, void (^handler)(id, NSError *)) {
    NSError *err = nil;
    id pso = xcmDev_newTilePSO(self, nullptr, desc, &err);
    if (handler) handler(pso, err);
}
static id xcmDev_newBufferWithDescriptor(id self, SEL, id desc) NS_RETURNS_RETAINED;
static id xcmDev_newBufferWithDescriptor(id self, SEL, id desc) {
    if (!gXm2vDevice || !desc) return nil;
    NSUInteger len = [desc respondsToSelector:@selector(length)] ? ((NSUInteger (*)(id, SEL))objc_msgSend)(desc, @selector(length)) : 0;
    if (len == 0) return nil;
    MTLResourceOptions opts = [desc respondsToSelector:@selector(resourceOptions)]
        ? ((MTLResourceOptions (*)(id, SEL))objc_msgSend)(desc, @selector(resourceOptions)) : MTLResourceStorageModeShared;
    void *contents = [desc respondsToSelector:@selector(contents)] ? ((void *(*)(id, SEL))objc_msgSend)(desc, @selector(contents)) : nullptr;
    BOOL noCopy = [desc respondsToSelector:@selector(noCopy)] ? ((BOOL (*)(id, SEL))objc_msgSend)(desc, @selector(noCopy)) : NO;
    if (contents && noCopy) {
        typedef void (^XcmBufDealloc)(void *, NSUInteger);
        XcmBufDealloc dealloc = nil;
        if ([desc respondsToSelector:@selector(deallocator)])
            dealloc = ((XcmBufDealloc (*)(id, SEL))objc_msgSend)(desc, @selector(deallocator));
        return xcmDev_newBufferNoCopy(self, nullptr, contents, len, opts, dealloc);
    }
    if (contents) return xcmDev_newBufferBytes(self, nullptr, contents, len, opts);
    return xcmDev_newBufferLen(self, nullptr, len, opts);
}

static Class createBackendDeviceClass(void) {
    static Class cls = nil;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Class base = objc_getClass("_MTLDevice");
        if (!base) base = [NSObject class];
        cls = objc_allocateClassPair(base, "XcmBackendDevice", 0);
        if (!cls) { cls = objc_getClass("XcmBackendDevice"); return; }

        struct Entry { const char *sel; IMP imp; const char *types; };
        static const Entry entries[] = {
            {"name", (IMP)xcmDev_name, "@16@0:8"},
            {"model", (IMP)xcmDev_name, "@16@0:8"},
            {"productName", (IMP)xcmDev_name, "@16@0:8"},
            {"targetDeviceArchitecture", (IMP)xcmDev_targetDeviceArchitecture, "@16@0:8"},
            {"registryID", (IMP)xcmDev_registryID, "Q16@0:8"},
            {"isLowPower", (IMP)xcmDev_isLowPower, "B16@0:8"},
            {"isHeadless", (IMP)xcmDev_isHeadless, "B16@0:8"},
            {"isRemovable", (IMP)xcmDev_isRemovable, "B16@0:8"},
            {"hasUnifiedMemory", (IMP)xcmDev_hasUnifiedMemory, "B16@0:8"},
            {"maxBufferLength", (IMP)xcmDev_maxBufferLength, "Q16@0:8"},
            {"maxThreadsPerThreadgroup", (IMP)xcmDev_maxThreadsPerThreadgroup, "{?=QQQ}16@0:8"},
            {"maxTextureWidth2D", (IMP)xcmDev_maxTex2D, "Q16@0:8"},
            {"maxTextureHeight2D", (IMP)xcmDev_maxTex2D, "Q16@0:8"},
            {"maxTextureWidth1D", (IMP)xcmDev_maxTex2D, "Q16@0:8"},
            {"maxTextureWidth3D", (IMP)xcmDev_maxTex3D, "Q16@0:8"},
            {"maxTextureHeight3D", (IMP)xcmDev_maxTex3D, "Q16@0:8"},
            {"maxTextureDepth3D", (IMP)xcmDev_maxTex3D, "Q16@0:8"},
            {"maxTextureLayers", (IMP)xcmDev_maxTexLayers, "Q16@0:8"},
            {"maxColorAttachments", (IMP)xcmDev_maxColorAtt, "Q16@0:8"},
            {"maxViewportCount", (IMP)xcmDev_maxViewportCount, "Q16@0:8"},
            {"maxArgumentBufferSamplerCount", (IMP)xcmDev_maxArgBufSamplers, "Q16@0:8"},
            {"maxThreadgroupMemoryLength", (IMP)xcmDev_maxTGMem, "Q16@0:8"},
            {"minimumLinearTextureAlignmentForPixelFormat:", (IMP)xcmDev_minLinearAlign, "Q24@0:8Q16"},
            {"minimumTextureBufferAlignmentForPixelFormat:", (IMP)xcmDev_minTileAlign, "Q24@0:8Q16"},
            {"linearTextureAlignmentBytes", (IMP)xcmDev_align16, "Q16@0:8"},
            {"iosurfaceTextureAlignmentBytes", (IMP)xcmDev_align16, "Q16@0:8"},
            {"iosurfaceReadOnlyTextureAlignmentBytes", (IMP)xcmDev_align16, "Q16@0:8"},
            {"deviceLinearTextureAlignmentBytes", (IMP)xcmDev_align16, "Q16@0:8"},
            {"deviceLinearReadOnlyTextureAlignmentBytes", (IMP)xcmDev_align16, "Q16@0:8"},
            {"readWriteTextureSupport", (IMP)xcmDev_rwTexTier, "Q16@0:8"},
            {"argumentBuffersSupport", (IMP)xcmDev_argBufTier, "Q16@0:8"},
            {"supportsSharedStorageTextures", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsRenderToLinearTextures", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsLinearTextureFromSharedBuffer", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsTextureSwizzle", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsNonPrivateDepthStencilTextures", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsUnalignedVertexFetch", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsBaseVertexInstanceDrawing", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsDepthClipMode", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsTextureCubeArray", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsNonUniformThreadgroupSize", (IMP)xcmDev_yes, "B16@0:8"},
            {"supportsProgrammableBlending", (IMP)xcmDev_yes, "B16@0:8"},
            {"isFramebufferReadSupported", (IMP)xcmDev_yes, "B16@0:8"},
            {"isDepth24Stencil8PixelFormatSupported", (IMP)xcmDev_no, "B16@0:8"},
            {"sparseTileSizeWithTextureType:pixelFormat:sampleCount:", (IMP)xcmDev_sparseTileSize, "{?=QQQ}40@0:8Q16Q24Q32"},
            {"sparseTileSizeWithTextureType:pixelFormat:sampleCount:sparsePageSize:", (IMP)xcmDev_sparseTileSizePage, "{?=QQQ}48@0:8Q16Q24Q32q40"},
            {"tileSizeWithSparsePageSize:textureType:pixelFormat:sampleCount:", (IMP)xcmDev_tileSizeSparse, "{?=QQQ}48@0:8q16Q24Q32Q40"},
            {"supportsTextureSampleCount:", (IMP)xcmDev_supportsSampleCount, "B24@0:8Q16"},
            {"supportsSampleCount:", (IMP)xcmDev_supportsSampleCount, "B24@0:8Q16"},
            {"supportsFeatureSet:", (IMP)xcmDev_supportsFeatureSet, "B24@0:8Q16"},
            {"supportsFamily:", (IMP)xcmDev_supportsFamily, "B24@0:8q16"},
            {"singleFPConfig", (IMP)xcmDev_singleFPConfig, "Q16@0:8"},
            {"doubleFPConfig", (IMP)xcmDev_zeroU, "Q16@0:8"},
            {"halfFPConfig", (IMP)xcmDev_halfFPConfig, "Q16@0:8"},
            {"metalAssertionsEnabled", (IMP)xcmDev_no, "B16@0:8"},
            {"setMetalAssertionsEnabled:", (IMP)xcmDev_noopBool, "v20@0:8B16"},
            {"copyIOSurfaceSharedTextureProperties:", (IMP)xcmDev_nil1, "@24@0:8@16"},
            {"mapShaderSampleBufferWithBuffer:capacity:size:", (IMP)xcmDev_noMapSample, "B40@0:8^{?=III}16Q24Q32"},
            {"unmapShaderSampleBuffer", (IMP)xcmDev_noop, "v16@0:8"},
            {"newBufferWithDescriptor:", (IMP)xcmDev_newBufferWithDescriptor, "@24@0:8@16"},
            {"newBufferWithIOSurface:", (IMP)xcmDev_nilPtr1, "@24@0:8^{__IOSurface=}16"},
            {"newIndirectCommandBufferWithDescriptor:maxCount:options:", (IMP)xcmDev_nilICB, "@40@0:8@16Q24Q32"},
            {"newIndirectComputeCommandEncoderWithBuffer:", (IMP)xcmDev_nil1, "@24@0:8@16"},
            {"newIndirectRenderCommandEncoderWithBuffer:", (IMP)xcmDev_nil1, "@24@0:8@16"},
            {"newRenderPipelineStateWithTileDescriptor:error:", (IMP)xcmDev_newTilePSO, "@32@0:8@16^@24"},
            {"newRenderPipelineStateWithTileDescriptor:completionHandler:", (IMP)xcmDev_newTilePSOAsync, "v32@0:8@16@?24"},
            {"newResourceGroupFromResources:count:", (IMP)xcmDev_nilResGrp, "@32@0:8r^@16Q24"},
            {"newTextureLayoutWithDescriptor:isHeapOrBufferBacked:", (IMP)xcmDev_nil2, "@28@0:8@16B24"},
            {"newTextureWithBytesNoCopy:length:descriptor:deallocator:", (IMP)xcmDev_nilTexNoCopy, "@48@0:8^v16Q24@32@?40"},
            {"newVisibleFunctionTableWithDescriptor:", (IMP)xcmDev_nil1, "@24@0:8@16"},
            {"reserveResourceIndicesForResourceType:indices:indexCount:", (IMP)xcmDev_noopReserve, "v40@0:8Q16^Q24Q32"},
            {"resourcePatchingTypeForResourceType:", (IMP)xcmDev_zeroU1, "Q24@0:8Q16"},
            {"newCommandQueue", (IMP)xcmDev_newCommandQueue, "@16@0:8"},
            {"newCommandQueueWithMaxCommandBufferCount:", (IMP)xcmDev_newCommandQueueMax, "@24@0:8Q16"},
            {"newBufferWithLength:options:", (IMP)xcmDev_newBufferLen, "@32@0:8Q16Q24"},
            {"newBufferWithBytes:length:options:", (IMP)xcmDev_newBufferBytes, "@40@0:8^v16Q24Q32"},
            {"newBufferWithBytesNoCopy:length:options:deallocator:", (IMP)xcmDev_newBufferNoCopy, "@48@0:8^v16Q24Q32@?40"},
            {"xcmNoCopyStats", (IMP)xcmDev_noCopyStats, "@16@0:8"},
            {"newTextureWithDescriptor:", (IMP)xcmDev_newTexture, "@24@0:8@16"},
            {"newTextureWithDescriptor:iosurface:plane:", (IMP)xcmDev_newTextureIOSurface, "@40@0:8@16^{__IOSurface=}24Q32"},
            {"newTextureWithDescriptor:iosurface:plane:slice:", (IMP)xcmDev_newTextureIOSurfaceSlice, "@48@0:8@16^{__IOSurface=}24Q32Q40"},
            {"newDefaultLibrary", (IMP)xcmDev_newDefaultLibrary, "@16@0:8"},
            {"newDefaultLibraryWithBundle:error:", (IMP)xcmDev_newDefaultLibraryWithBundle, "@32@0:8@16^@24"},
            {"newLibraryWithSource:options:error:", (IMP)xcmDev_newLibraryWithSource, "@40@0:8@16@24^@32"},
            {"newLibraryWithSource:options:completionHandler:", (IMP)xcmDev_newLibraryWithSourceAsync, "v40@0:8@16@24@?32"},
            {"newLibraryWithData:error:", (IMP)xcmDev_newLibraryWithData, "@32@0:8@16^@24"},
            {"newLibraryWithURL:error:", (IMP)xcmDev_newLibraryWithURL, "@32@0:8@16^@24"},
            {"newLibraryWithFile:error:", (IMP)xcmDev_newLibraryWithFile, "@32@0:8@16^@24"},
            {"newLibraryWithStitchedDescriptor:error:", (IMP)xcmDev_newLibraryWithStitched, "@32@0:8@16^@24"},
            {"newLibraryWithStitchedDescriptor:destinationBinaryArchive:error:", (IMP)xcmDev_newLibraryWithStitchedArchive, "@40@0:8@16@24^@32"},
            {"newLibraryWithStitchedDescriptor:completionHandler:", (IMP)xcmDev_newLibraryWithStitchedAsync, "v32@0:8@16@?24"},
            {"newComputePipelineStateWithFunction:error:", (IMP)xcmDev_newComputePSO, "@32@0:8@16^@24"},
            {"newComputePipelineStateWithFunction:options:reflection:error:", (IMP)xcmDev_newComputePSOOpts, "@48@0:8@16Q24^@32^@40"},
            {"newComputePipelineStateWithDescriptor:options:reflection:error:", (IMP)xcmDev_newComputePSODesc, "@48@0:8@16Q24^@32^@40"},
            {"newComputePipelineStateWithFunction:completionHandler:", (IMP)xcmDev_newComputePSOAsync, "v32@0:8@16@?24"},
            {"newRenderPipelineStateWithDescriptor:error:", (IMP)xcmDev_newRenderPSO, "@32@0:8@16^@24"},
            {"newRenderPipelineStateWithDescriptor:options:reflection:error:", (IMP)xcmDev_newRenderPSOOpts, "@48@0:8@16Q24^@32^@40"},
            {"newSamplerStateWithDescriptor:", (IMP)xcmDev_newSampler, "@24@0:8@16"},
            {"newDepthStencilStateWithDescriptor:", (IMP)xcmDev_newDepthStencil, "@24@0:8@16"},
            {"newEvent", (IMP)xcmDev_newEvent, "@16@0:8"},
            {"newSharedEvent", (IMP)xcmDev_newSharedEvent, "@16@0:8"},
            {"newSharedEventWithOptions:", (IMP)xcmDev_newSharedEventOpts, "@24@0:8Q16"},
            {"newSharedEventWithHandle:", (IMP)xcmDev_newSharedEventWithHandle, "@24@0:8@16"},
            {"newFence", (IMP)xcmDev_newFence, "@16@0:8"},
            {"newHeapWithDescriptor:", (IMP)xcmDev_newHeap, "@24@0:8@16"},
            {"heapTextureSizeAndAlignWithDescriptor:", (IMP)xcmDev_heapTexSizeAndAlign, "{?=QQ}24@0:8@16"},
            {"heapBufferSizeAndAlignWithLength:options:", (IMP)xcmDev_heapBufSizeAndAlign, "{?=QQ}32@0:8Q16Q24"},
        };
        for (const auto &e : entries)
            class_addMethod(cls, sel_registerName(e.sel), e.imp, e.types);
        Protocol *mtlDevProto = objc_getProtocol("MTLDevice");
        if (mtlDevProto) class_addProtocol(cls, mtlDevProto);
        // MTLAddDevice asserts [newDevice conformsToProtocol:@protocol(MTLDeviceSPI)] (Metal's own device list).
        Protocol *mtlDevSPI = objc_getProtocol("MTLDeviceSPI");
        if (!mtlDevSPI) fprintf(stderr, "XnuCpuMetal: no MTLDeviceSPI protocol in the runtime\n");
        if (mtlDevSPI) class_addProtocol(cls, mtlDevSPI);
        objc_registerClassPair(cls);
    });
    return cls;
}

static void fillBaseLimits(id device) {
    Class base = objc_getClass("_MTLDevice");
    SEL limitsSel = sel_registerName("limits");
    if (!base || !device || ![device isKindOfClass:base] || !class_getInstanceMethod(base, limitsSel)) return;
    uint8_t *limits = ((uint8_t *(*)(id, SEL))objc_msgSend)(device, limitsSel);
    ptrdiff_t at = limits - (uint8_t *)(__bridge void *)device;
    ptrdiff_t instance = (ptrdiff_t)class_getInstanceSize(base);
    if (!limits || at <= 0 || at >= instance) return;
    ptrdiff_t end = instance;
    bool found = false;
    unsigned count = 0;
    Ivar *ivars = class_copyIvarList(base, &count);
    for (unsigned i = 0; i < count; i++) {
        ptrdiff_t off = ivar_getOffset(ivars[i]);
        if (off == at) found = true;
        else if (off > at && off < end) end = off;
    }
    free(ivars);
    if (!found || end - at < 8) return;
    size_t size = (size_t)(end - at);
    struct LimitVal { const char *sel; uint64_t val; };
    const LimitVal kLimits[] = {
        {"maxTextureWidth1D", 16384},
        {"maxTextureWidth2D", 16384},
        {"maxTextureHeight2D", 16384},
        {"maxTextureWidth3D", 2048},
        {"maxTextureHeight3D", 2048},
        {"maxTextureDepth3D", 2048},
        {"maxTextureLayers", 2048},
        {"maxColorAttachments", 8},
        {"maxThreadgroupMemoryLength", 32768},
        {"maxBufferLength", xcmDev_maxBufferLength(device, nullptr)},
        {"maxFramebufferStorageBits", 512},
        {"maxTotalComputeThreadsPerThreadgroup", 1024},
        {"maxComputeSamplers", 16},
        {"maxFragmentSamplers", 16},
        {"maxVertexSamplers", 16},
        {"maxComputeTextures", 128},
        {"maxFragmentTextures", 128},
        {"maxVertexTextures", 128},
        {"maxComputeBuffers", 31},
        {"maxFragmentBuffers", 31},
        {"maxVertexBuffers", 31},
        {"maxVertexAttributes", 31},
        {"maxInterpolants", 32},
        {"maxViewports", 16},
        {"maxCustomSamplePositions", 0},
        {"linearTextureAlignmentBytes", 64},
        {"linearTextureArrayAlignmentBytes", 16},
        {"linearTextureArrayAlignmentSlice", 128},
        {"minConstantBufferAlignmentBytes", 4},
        {"minBufferNoCopyAlignmentBytes", 4096},
        {"maxComputeThreadgroupMemoryAlignmentBytes", 16},
    };
    for (const auto &lv : kLimits) {
        SEL s = sel_registerName(lv.sel);
        Method orig = class_getInstanceMethod(base, s);
        if (!orig || method_getNumberOfArguments(orig) != 2) continue;
        char ret = method_getTypeEncoding(orig)[0];
        if (ret != 'Q' && ret != 'I') continue;
        auto getter = (NSUInteger (*)(id, SEL))method_getImplementation(orig);
        for (size_t off = 0; off + 4 <= size; off += 4) {
            uint32_t saved;
            memcpy(&saved, limits + off, 4);
            uint32_t marker = 0x5a17c000u | (uint32_t)(off & 0xfff);
            memcpy(limits + off, &marker, 4);
            NSUInteger seen = getter(device, s);
            memcpy(limits + off, &saved, 4);
            if ((uint32_t)seen != marker || (seen >> 32) != 0) continue;
            bool wide = false;
            if (off + 8 <= size) {
                uint32_t next, one = 1;
                memcpy(&next, limits + off + 4, 4);
                memcpy(limits + off, &marker, 4);
                memcpy(limits + off + 4, &one, 4);
                wide = (getter(device, s) >> 32) == 1;
                memcpy(limits + off, &saved, 4);
                memcpy(limits + off + 4, &next, 4);
            }
            if (wide) {
                memcpy(limits + off, &lv.val, 8);
            } else {
                uint32_t narrow = lv.val > UINT32_MAX ? UINT32_MAX : (uint32_t)lv.val;
                memcpy(limits + off, &narrow, 4);
            }
            break;
        }
    }
}

static void *xcmLoadDirectDriver(const std::string &dylibPath) {
    if (dylibPath.empty()) return nullptr;
    void *handle = dlopen(dylibPath.c_str(), RTLD_NOW | RTLD_LOCAL);
    if (!handle) {
        const char *err = dlerror();
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: dlopen(%{public}s) failed: %{public}s",
                     dylibPath.c_str(), err ?: "?");
        return nullptr;
    }
    // VK_LUNARG_direct_driver_loading requires ICD loader interface version >= 7
    // (Mesa lavapipe and Venus support v7; older ICDs like MoltenVK 1.4.2 report v5
    // and fall back to manifest-based loading).
    uint32_t ver = 7;
    auto negotiate = (int32_t (*)(uint32_t *))dlsym(handle, "vk_icdNegotiateLoaderICDInterfaceVersion");
    if (!negotiate || negotiate(&ver) != 0 || ver < 7) {
        return nullptr;
    }
    void *pfn = dlsym(handle, "vk_icdGetInstanceProcAddr");
    if (!pfn) {
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: dlsym(vk_icdGetInstanceProcAddr) in %{public}s failed",
                     dylibPath.c_str());
        return nullptr;
    }
    return pfn;
}

static std::string xcmResolveIcdLibraryPath(const std::string &icdJsonPath) {
    if (icdJsonPath.empty() || icdJsonPath.find(':') != std::string::npos) return {};
    NSData *data = [NSData dataWithContentsOfFile:@(icdJsonPath.c_str())];
    if (!data) return {};
    id obj = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![obj isKindOfClass:[NSDictionary class]]) return {};
    id icd = ((NSDictionary *)obj)[@"ICD"];
    if (![icd isKindOfClass:[NSDictionary class]]) return {};
    id libPath = ((NSDictionary *)icd)[@"library_path"];
    if (![libPath isKindOfClass:[NSString class]] || [(NSString *)libPath length] == 0) return {};
    NSString *lp = (NSString *)libPath;
    if ([lp hasPrefix:@"/"]) return lp.fileSystemRepresentation;
    NSString *dir = [@(icdJsonPath.c_str()) stringByDeletingLastPathComponent];
    NSString *full = [[dir stringByAppendingPathComponent:lp] stringByStandardizingPath];
    return full.fileSystemRepresentation;
}

extern "C" id xnu_cpu_metal_open_device_direct(const char *loader, void *direct_driver_pfn,
                                               const char *icd_json, const char *cache_dir, unsigned port) {
    if (gSingletonDevice) return gSingletonDevice;
    gXm2vDevice = xm2v_device_open_direct(loader, direct_driver_pfn, icd_json, cache_dir);
    if (!gXm2vDevice) {
        const char *err = xm2v_last_error();
        fprintf(stderr, "XnuCpuMetal: xm2v_device_open failed (cache=%s): %s\n", cache_dir ?: "(null)", err ?: "?");
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: xm2v_device_open failed (prog=%{public}s uid=%u cache=%{public}s): %{public}s",
                     getprogname(), (unsigned)getuid(), cache_dir ?: "(null)", err ?: "?");
        return nil;
    }
    if (port) {
        uint64_t rid = 0;
        if (IORegistryEntryGetRegistryEntryID(port, &rid) == KERN_SUCCESS && rid != 0)
            gRegistryID = rid;
    }
    Class cls = createBackendDeviceClass();
    id dev = [[cls alloc] init];
    fillBaseLimits(dev);
    xcm_iogpu::install(dev, port);
    gSingletonDevice = dev;
    return dev;
}

extern "C" id xnu_cpu_metal_open_device(const char *loader, const char *icd_json, const char *cache_dir, unsigned port) {
    return xnu_cpu_metal_open_device_direct(loader, nullptr, icd_json, cache_dir, port);
}

/* ======================== Bundle Principal Class ======================== */

@interface XnuCpuMetalDevice : IOGPUMetalDevice
- (id)initWithAcceleratorPort:(unsigned)port;
- (id)initWithAcceleratorPort:(unsigned)port options:(unsigned long long)options;
@end

@implementation XnuCpuMetalDevice

- (id)initWithAcceleratorPort:(unsigned)port options:(unsigned long long)options {
    (void)options;
    return [self initWithAcceleratorPort:port];
}

- (id)initWithAcceleratorPort:(unsigned)port {
    static NSMutableArray *kept;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ kept = [NSMutableArray array]; });
    @synchronized(kept) { [kept addObject:self]; }

    Dl_info info = {};
    std::string contentsDir;
    if (dladdr((const void *)&xnu_cpu_metal_open_device, &info) && info.dli_fname) {
        std::string execPath(info.dli_fname);
        size_t pos = execPath.rfind("/MacOS/");
        if (pos != std::string::npos) contentsDir = execPath.substr(0, pos);
    }

    std::string loader = contentsDir.empty() ? "libvulkan.1.dylib" : (contentsDir + "/Frameworks/libvulkan.1.dylib");
    std::string lvpIcd = contentsDir.empty() ? "" : (contentsDir + "/Resources/lvp_icd.aarch64.json");
    std::string venusIcd = contentsDir.empty() ? "" : (contentsDir + "/Resources/virtio_icd.aarch64.json");
    std::string lvpDylib = contentsDir.empty() ? "" : (contentsDir + "/Frameworks/libvulkan_lvp.dylib");
    std::string venusDylib = contentsDir.empty() ? "" : (contentsDir + "/Frameworks/libvulkan_virtio.dylib");

    // The overrides name code to load into the process: never honoured where the environment is not the process's own
    // (set-id programs), as dyld ignores DYLD_* there.
    auto xcmEnv = [](const char *name) -> const char * { return issetugid() ? nullptr : getenv(name); };
    const char *envLoader = xcmEnv("XNU_CPU_METAL_LOADER");
    if (envLoader && envLoader[0]) loader = envLoader;
    const char *envDriver = xcmEnv("XNU_CPU_METAL_DRIVER");
    const char *envIcd = xcmEnv("XNU_CPU_METAL_ICD");
    const char *envBackend = xcmEnv("XNU_CPU_METAL_BACKEND");

    bool useVenus = false;
    bool forcedBackend = false;
    bool customDriver = (envDriver && envDriver[0]) || (envIcd && envIcd[0]);
    std::string directDylib;
    std::string icdJson;
    if (envDriver && envDriver[0]) {
        directDylib = envDriver;
    } else if (envIcd && envIcd[0]) {
        icdJson = envIcd;
        // Single-ICD JSON: resolve library_path and load via VK_LUNARG_direct_driver_loading
        // (works in secure mode and avoids scanning icd.d). Multi-ICD colon lists fall back to env.
        directDylib = xcmResolveIcdLibraryPath(icdJson);
    } else {
        if (envBackend && strcasecmp(envBackend, "venus") == 0) {
            useVenus = true;
            forcedBackend = true;
        } else if (envBackend && strcasecmp(envBackend, "lavapipe") == 0) {
            useVenus = false;
            forcedBackend = true;
        } else {
            io_service_t drmSvc = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("XnuDrm"));
            if (drmSvc) {
                IOObjectRelease(drmSvc);
                struct stat st = {};
                if (!venusDylib.empty() && stat(venusDylib.c_str(), &st) == 0) useVenus = true;
            }
        }
        directDylib = useVenus ? venusDylib : lvpDylib;
        icdJson = useVenus ? venusIcd : lvpIcd;
    }

    // IOSurfaces read/written by this Metal device must be linear uncompressed rows (WindowServer/QuartzCore).
    // Use in-memory NSRegistrationDomain (never persisted to any app's plist on disk).
    [[NSUserDefaults standardUserDefaults] registerDefaults:@{@"CAEnableIOSurfaceCompression": @NO}];

    char cacheDir[512] = {};
    auto tryCacheDir = [&](const char *path) -> bool {
        if (!path || !path[0]) return false;
        mkdir(path, 0700);
        // Verify actual file-creation permission (matches metal2vulkan ShaderCache write probe;
        // prevents WindowServer uid=88 from failing on root-owned or sandboxed directories).
        char probe[560];
        snprintf(probe, sizeof(probe), "%s/.xcm_probe_%d", path, (int)getpid());
        int fd = open(probe, O_CREAT | O_WRONLY | O_TRUNC, 0600);
        if (fd >= 0) {
            close(fd);
            unlink(probe);
            strlcpy(cacheDir, path, sizeof(cacheDir));
            return true;
        }
        return false;
    };
    const char *envCache = getenv("XNU_CPU_METAL_CACHE_DIR");
    if (!tryCacheDir(envCache)) {
        char darwinCache[512] = {};
        bool ok = false;
        if (confstr(_CS_DARWIN_USER_CACHE_DIR, darwinCache, sizeof(darwinCache) - 32) > 0) {
            strlcat(darwinCache, "xnu-cpu-metal-m2v", sizeof(darwinCache));
            ok = tryCacheDir(darwinCache);
        }
        if (!ok && confstr(_CS_DARWIN_USER_TEMP_DIR, darwinCache, sizeof(darwinCache) - 32) > 0) {
            strlcat(darwinCache, "xnu-cpu-metal-m2v", sizeof(darwinCache));
            ok = tryCacheDir(darwinCache);
        }
        if (!ok) {
            NSString *tmp = NSTemporaryDirectory();
            if (tmp.length > 0) {
                snprintf(darwinCache, sizeof(darwinCache), "%s/xnu-cpu-metal-%u", tmp.fileSystemRepresentation, (unsigned)getuid());
                ok = tryCacheDir(darwinCache);
            }
        }
        if (!ok) {
            char fallback[128];
            snprintf(fallback, sizeof(fallback), "/var/tmp/xnu-cpu-metal-%u", (unsigned)getuid());
            ok = tryCacheDir(fallback);
        }
        if (!ok) {
            char fallback[128];
            snprintf(fallback, sizeof(fallback), "/tmp/xnu-cpu-metal-%u", (unsigned)getuid());
            tryCacheDir(fallback);
        }
    }

    auto configureLavapipeEnv = [&]() {
        // Configure Mesa lavapipe LLVM JIT disk cache (bounded, overridable by caller).
        if (cacheDir[0]) {
            char mesaCache[544];
            snprintf(mesaCache, sizeof(mesaCache), "%s/mesa", cacheDir);
            mkdir(mesaCache, 0700);
            setenv("MESA_SHADER_CACHE_DIR", mesaCache, 0);
        }
        setenv("MESA_SHADER_CACHE_MAX_SIZE", "256M", 0);

        // Multi-VM safe lavapipe thread default: 0 (inline, no worker threads) on 1-CPU hosts,
        // capped at 2 on multi-core hosts; always overridable via LP_NUM_THREADS.
        if (!getenv("LP_NUM_THREADS")) {
            int ncpu = 1;
            size_t sz = sizeof(ncpu);
            if (sysctlbyname("hw.ncpu", &ncpu, &sz, nullptr, 0) != 0 || ncpu < 1) ncpu = 1;
            int lpThreads = (ncpu <= 1) ? 0 : (ncpu > 2 ? 2 : ncpu);
            char buf[16];
            snprintf(buf, sizeof(buf), "%d", lpThreads);
            setenv("LP_NUM_THREADS", buf, 0);
        }
    };

    if (!customDriver && !useVenus) configureLavapipeEnv();

    void *directPfn = directDylib.empty() ? nullptr : xcmLoadDirectDriver(directDylib);
    id dev = nil;
    if (directPfn || !icdJson.empty()) {
        dev = xnu_cpu_metal_open_device_direct(loader.c_str(), directPfn, icdJson.c_str(), cacheDir, port);
    }
    if (!dev && useVenus && !forcedBackend && !customDriver && !lvpDylib.empty()) {
        os_log_error(OS_LOG_DEFAULT, "XnuCpuMetal: Venus backend open failed (%{public}s), falling back to lavapipe",
                     xm2v_last_error() ?: "?");
        configureLavapipeEnv();
        void *lvpPfn = xcmLoadDirectDriver(lvpDylib);
        dev = xnu_cpu_metal_open_device_direct(loader.c_str(), lvpPfn, lvpIcd.c_str(), cacheDir, port);
    }
    return dev;
}

@end

// SPDX-License-Identifier: APSL-2.0
// XnuEAN: the EAN of the hosted macOS, the small keyed store the Secure Enclave's replacement on Apple's virtual platform keeps
// its xART in.
//
// The collection is VirtualMac's: AppleVPKeyStore and AppleVPCredentialManager are the keystore in software, and
// AppleVirtualPlatformXART (AppleVirtualPlatform) derives, seals and loads the xART seeds (the SEP's anti-replay storage).
// It reads and writes them as two blobs, keys 'xrtC' and 'xrtP', of an EAN: it waits forever for an IOService named
// "AppleNVMeEAN", casts it to that class (AppleVirtIOStorage's), and calls AppleNVMeEAN::eanGetSize / eanRead / eanWrite /
// eanSwap on it directly. On a VirtualMac that object sits on the auxiliary disk of AppleVirtIOStorageDevice; the hosted
// system has no such device, and without an EAN the keystore's first open (applekeystored, powerd at boot) blocks forever.
//
// Here: an AppleNVMeEAN instance (never started: it has no device) is registered so the cast succeeds, and the four entry
// points of the class are redirected to the functions below (a branch written over each one's first instruction). The blobs
// live in memory and are written through to a file on the Linux side (xnu.ko keystore=, linux64_store_rw in bsd/kern/linux64_host.c),
// so they last as long as that file does. Nothing else on this system uses AppleNVMeEAN.
#include <IOKit/IOLib.h>
#include <IOKit/IOLocks.h>
#include <IOKit/IOService.h>
#include <kern/thread.h>
#include <libkern/OSByteOrder.h>
#include <ptrauth.h>

extern "C" long linux64_store_rw(int write, void *buffer, unsigned long length, unsigned long offset);
// the kernel's own (osfmk), under names of ours: the kernel headers some of them come from disagree on their types
extern "C" uint64_t xnuKvtophys(vm_offset_t va) __asm("_kvtophys");
extern "C" void xnuPhysWriteWord(uint64_t paddr, unsigned int data) __asm("_ml_phys_write_word_64");
extern "C" void xnuInvalidateIcache(vm_offset_t va, size_t length) __asm("_InvalidatePoU_IcacheRegion");

// AppleVirtIOStorage's AppleNVMeEAN (non-virtual members; AppleVirtualPlatform calls them with bl)
extern "C" void appleEanGetSize(void) __asm("__ZN12AppleNVMeEAN10eanGetSizeEj");
extern "C" void appleEanRead(void) __asm("__ZN12AppleNVMeEAN7eanReadEjPvj");
extern "C" void appleEanWrite(void) __asm("__ZN12AppleNVMeEAN8eanWriteEjPvj");
extern "C" void appleEanSwap(void) __asm("__ZN12AppleNVMeEAN7eanSwapEjj");
// AppleVirtualPlatform's sealed registers (non-virtual; AppleVPKeyStore calls it on the instance it waits for at late init)
extern "C" void appleSealedRegisterUpdate(void) __asm("__ZN35AppleVirtualPlatformSealedRegisters20sealedRegisterUpdateEPKhyS1_m");

namespace {
constexpr uint32_t kMagic = 'XEAN', kMaxEntries = 16, kMaxBlob = 64 * 1024, kStoreMax = 1024 * 1024;
struct Entry { uint32_t key, length; uint8_t *data; };
Entry gEntries[kMaxEntries];
IOLock *gLock;
uint8_t *gImage;   // the store file's contents: magic, count, then {key, length, bytes} per entry

Entry *find(uint32_t key)
{
    for (auto &e : gEntries)
        if (e.data && e.key == key)
            return &e;
    return nullptr;
}

void persist()
{
    uint32_t at = 8, count = 0;
    for (auto &e : gEntries) {
        if (!e.data || at + 8 + e.length > kStoreMax)
            continue;
        memcpy(gImage + at, &e.key, 4);
        memcpy(gImage + at + 4, &e.length, 4);
        memcpy(gImage + at + 8, e.data, e.length);
        at += 8 + e.length;
        count++;
    }
    memcpy(gImage, &kMagic, 4);
    memcpy(gImage + 4, &count, 4);
    long done = linux64_store_rw(1, gImage, (at + 4095) & ~4095u, 0);
    if (done < (long)at)
        IOLog("XnuEAN: writing the store failed (%ld)\n", done);
}

void load()
{
    long got = linux64_store_rw(0, gImage, kStoreMax, 0);
    uint32_t magic = 0, count = 0;
    if (got >= 8) {
        memcpy(&magic, gImage, 4);
        memcpy(&count, gImage + 4, 4);
    }
    if (magic != kMagic) {
        IOLog("XnuEAN: empty store (%ld bytes read)\n", got);
        return;
    }
    uint32_t at = 8;
    for (uint32_t i = 0; i < count && i < kMaxEntries && at + 8 <= (uint64_t)got; i++) {
        Entry &e = gEntries[i];
        memcpy(&e.key, gImage + at, 4);
        memcpy(&e.length, gImage + at + 4, 4);
        if (e.length > kMaxBlob || at + 8 + e.length > (uint64_t)got)
            break;
        e.data = (uint8_t *)IOMallocData(e.length ? e.length : 1);
        memcpy(e.data, gImage + at + 8, e.length);
        at += 8 + e.length;
    }
    IOLog("XnuEAN: %u blobs loaded from the store\n", count);
}
}

// The replacements, with AppleNVMeEAN's signatures (this first). Return values as AppleVirtualPlatformXART reads them: the
// blob's size, 0 when there is none; kIOReturnSuccess or an error.
extern "C" uint32_t xnu_ean_get_size(void *, uint32_t key)
{
    IOLockLock(gLock);
    Entry *e = find(key);
    uint32_t size = e ? e->length : 0;
    IOLockUnlock(gLock);
    return size;
}

extern "C" IOReturn xnu_ean_read(void *, uint32_t key, void *buffer, uint32_t length)
{
    IOLockLock(gLock);
    Entry *e = find(key);
    IOReturn result = kIOReturnNotFound;
    if (e && buffer) {
        memcpy(buffer, e->data, length < e->length ? length : e->length);
        result = kIOReturnSuccess;
    }
    IOLockUnlock(gLock);
    return result;
}

extern "C" IOReturn xnu_ean_write(void *, uint32_t key, void *buffer, uint32_t length)
{
    if (length > kMaxBlob || (length && !buffer))
        return kIOReturnBadArgument;
    uint8_t *copy = (uint8_t *)IOMallocData(length ? length : 1);
    if (!copy)
        return kIOReturnNoMemory;
    memcpy(copy, buffer, length);
    IOLockLock(gLock);
    Entry *e = find(key);
    if (!e)
        for (auto &slot : gEntries)
            if (!slot.data) {
                e = &slot;
                break;
            }
    IOReturn result = kIOReturnNoSpace;
    if (e) {
        if (e->data)
            IOFreeData(e->data, e->length ? e->length : 1);
        e->key = key;
        e->length = length;
        e->data = copy;
        copy = nullptr;
        persist();
        result = kIOReturnSuccess;
    }
    IOLockUnlock(gLock);
    if (copy)
        IOFreeData(copy, length ? length : 1);
    return result;
}

extern "C" IOReturn xnu_ean_swap(void *, uint32_t a, uint32_t b)
{
    IOLockLock(gLock);
    Entry *ea = find(a), *eb = find(b);
    if (ea)
        ea->key = b;
    if (eb)
        eb->key = a;
    if (ea || eb)
        persist();
    IOLockUnlock(gLock);
    return kIOReturnSuccess;
}

// AppleVirtualPlatformSealedRegisters::sealedRegisterUpdate(const uint8_t *uuid, uint64_t, const uint8_t *bytes, size_t):
// on a VirtualMac it passes the value to the host (Virtualization.framework) to fold into a register the VM cannot read
// back; the hosted system has no such host side and nothing reads the registers, so an update is accepted as made.
extern "C" uint32_t xnu_sealed_register_update(void *, const uint8_t *, uint64_t, const uint8_t *, size_t length)
{
    return length <= 512;
}

class XnuEAN : public IOService {
    OSDeclareDefaultStructors(XnuEAN)
    IOService *fEan = nullptr, *fSealed = nullptr;
    static bool redirect(void (*from)(void), const void *to, const char *what);
    static void publishSealedRegisters(void *self, wait_result_t);

public:
    bool start(IOService *provider) override;
};

OSDefineMetaClassAndStructors(XnuEAN, IOService)

// A branch to `to` over the first instruction of `from` (its callers enter with bl, which a BTI landing pad does not need).
bool XnuEAN::redirect(void (*from)(void), const void *to, const char *what)
{
    uintptr_t src = (uintptr_t)ptrauth_strip((void *)from, ptrauth_key_function_pointer);
    uintptr_t dst = (uintptr_t)ptrauth_strip((void *)to, ptrauth_key_function_pointer);
    intptr_t delta = (intptr_t)(dst - src);
    if (delta & 3 || delta >= (1 << 27) || delta < -(1 << 27)) {
        IOLog("XnuEAN: %s is out of branch range (%p -> %p)\n", what, (void *)src, (void *)dst);
        return false;
    }
    uint64_t pa = xnuKvtophys(src);
    if (!pa)
        return false;
    xnuPhysWriteWord(pa, 0x14000000u | (((uint32_t)(delta >> 2)) & 0x03ffffffu));
    xnuInvalidateIcache(src, 4);
    return true;
}

bool XnuEAN::start(IOService *provider)
{
    if (!IOService::start(provider))
        return false;
    gLock = IOLockAlloc();
    gImage = (uint8_t *)IOMallocData(kStoreMax);
    if (!gLock || !gImage)
        return false;
    load();
    if (!redirect(appleEanGetSize, (const void *)&xnu_ean_get_size, "eanGetSize") ||
        !redirect(appleEanRead, (const void *)&xnu_ean_read, "eanRead") ||
        !redirect(appleEanWrite, (const void *)&xnu_ean_write, "eanWrite") ||
        !redirect(appleEanSwap, (const void *)&xnu_ean_swap, "eanSwap"))
        return false;
    // an instance of the class AppleVirtualPlatformXART casts to, registered under its class name (its IOName)
    OSObject *object = OSMetaClass::allocClassWithName("AppleNVMeEAN");
    fEan = OSDynamicCast(IOService, object);
    if (!fEan || !fEan->init(nullptr) || !fEan->attach(this)) {
        IOLog("XnuEAN: cannot make an AppleNVMeEAN (%p)\n", object);
        OSSafeReleaseNULL(object);
        fEan = nullptr;
        return false;
    }
    fEan->registerService();
    // the virtual platform's sealed registers, which AppleVPKeyStore's late init waits for forever (holding the keystore's
    // gate, so every AppleKeyStore open blocks behind it). Published only once AppleKeyStore itself is: present while the
    // keystore starts, the instance (never started) sends its start down another path that never publishes the service.
    if (redirect(appleSealedRegisterUpdate, (const void *)&xnu_sealed_register_update, "sealedRegisterUpdate")) {
        thread_t thread;
        retain();
        if (kernel_thread_start(&XnuEAN::publishSealedRegisters, this, &thread) == KERN_SUCCESS)
            thread_deallocate(thread);
        else
            release();
    }
    registerService();
    IOLog("XnuEAN: AppleNVMeEAN registered; its EAN calls go to the hosted store\n");
    return true;
}

void XnuEAN::publishSealedRegisters(void *arg, wait_result_t)
{
    XnuEAN *self = (XnuEAN *)arg;
    OSDictionary *match = IOService::serviceMatching("AppleKeyStore");
    IOService *aks = match ? IOService::waitForMatchingService(match) : nullptr;
    OSSafeReleaseNULL(match);
    OSSafeReleaseNULL(aks);
    OSObject *sealed = OSMetaClass::allocClassWithName("AppleVirtualPlatformSealedRegisters");
    self->fSealed = OSDynamicCast(IOService, sealed);
    if (self->fSealed && self->fSealed->init(nullptr) && self->fSealed->attach(self)) {
        self->fSealed->registerService();
        IOLog("XnuEAN: AppleVirtualPlatformSealedRegisters registered (after AppleKeyStore)\n");
    } else {
        IOLog("XnuEAN: cannot make an AppleVirtualPlatformSealedRegisters (%p)\n", sealed);
        OSSafeReleaseNULL(sealed);
        self->fSealed = nullptr;
    }
    self->release();
    thread_terminate(current_thread());
}

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

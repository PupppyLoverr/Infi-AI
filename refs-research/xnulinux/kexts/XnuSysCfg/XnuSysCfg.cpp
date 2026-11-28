// SPDX-License-Identifier: APSL-2.0
// XnuSysCfg: the service MobileGestalt reads the machine's system configuration (syscfg) from, with nothing in it.
//
// libMobileGestalt, in every process that asks it for a syscfg key (AppKit does while NSApplication starts, for the
// system appearance), looks for the syscfg in /chosen (syscfg-v2-data), then for the IOService
// AppleDiagnosticDataAccessReadOnly, and when there is none waits five seconds for one to appear ("giving up waiting for
// match - runloop result is 3") before answering that the key is not there. The hosted system has neither, so every
// application spent its first five seconds in that wait. This is that service's name with no syscfg behind it and no
// user client: the lookup finds it at once, fails to read from it, and MobileGestalt gives the same answer without waiting.
#include <IOKit/IOLib.h>
#include <IOKit/IOService.h>

class AppleDiagnosticDataAccessReadOnly : public IOService {
    OSDeclareDefaultStructors(AppleDiagnosticDataAccessReadOnly)

public:
    bool start(IOService *provider) override
    {
        if (!IOService::start(provider))
            return false;
        setName("AppleDiagnosticData");
        registerService();
        return true;
    }
};

OSDefineMetaClassAndStructors(AppleDiagnosticDataAccessReadOnly, IOService)

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

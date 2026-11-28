// SPDX-License-Identifier: APSL-2.0
// XnuCpuMetal: the hosted macOS's Metal device, as a kext of the kernel collection (written for xnulinux, clean room).
// Metal.framework finds GPUs by watching IOServiceMatching("IOAccelerator") and loads the plugin bundle named by the service's
// MetalPluginName from /Library/GPUBundles, taking MetalPluginClassName (an IOGPUMetalDevice subclass) as the device class.
// XnuCpuMetalDevice runs its work in the calling process (Metal -> metal2vulkan -> Mesa), so the service is only a registry
// entry. It sits on the device tree's sgx node: System Information describes the GPU from that node's first child.
#include <IOKit/IOService.h>
#include <IOKit/IOLib.h>
#include <mach/kmod.h>

// IOGraphicsFamily's abstract accelerator class; scripts/kclink fills the subclass's vtable from it (the JSON's "vtable").
class IOAccelerator : public IOService {
	OSDeclareAbstractStructors(IOAccelerator)
};

class XnuCpuMetalAccelerator : public IOAccelerator {
	OSDeclareDefaultStructors(XnuCpuMetalAccelerator)
public:
	bool start(IOService *provider) override;
};

OSDefineMetaClassAndStructors(XnuCpuMetalAccelerator, IOAccelerator)

bool XnuCpuMetalAccelerator::start(IOService *provider)
{
	if (!IOService::start(provider)) return false;
	setProperty("MetalPluginName", "XnuCpuMetal");
	setProperty("MetalPluginClassName", "XnuCpuMetalDevice");
	// No "Removable" / "SafeEjectRequested": Metal (-[MTLIOAccelService initWithAcceleratorPort:deviceClass:]) skips
	// a service on which either is present at all, whatever the value.
	{
		// the host GPU's name (the node's "model") and its PCI vendor
		char name[64] = "XnuCpuMetal (lavapipe)";
		OSData *m = OSDynamicCast(OSData, provider->getProperty("model"));
		if (m && m->getLength() > 1 && m->getLength() <= sizeof(name)) {
			memcpy(name, m->getBytesNoCopy(), m->getLength());
			name[sizeof(name) - 1] = 0;
		}
		setProperty("model", name);
		uint32_t vendor = !strncmp(name, "NVIDIA", 6) ? 0x10de : !strncmp(name, "AMD", 3) ? 0x1002 :
		    !strncmp(name, "Intel", 5) ? 0x8086 : !strncmp(name, "Apple", 5) ? 0x106b : 0;
		if (vendor) setProperty("vendor-id", &vendor, sizeof(vendor));
	}
	// The GPU bridge's user client (kexts/XnuDrm) hangs off this service: the sandbox's system-graphics rules let
	// processes that draw open user clients of an IOAccelerator ((iokit-connection "IOAccelerator")), whatever their class.
	setProperty("IOUserClientClass", "AGXDeviceUserClient");
	registerService();
	IOLog("xnucpumetal: Metal accelerator published on %s\n", provider->getName());
	return true;
}

extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

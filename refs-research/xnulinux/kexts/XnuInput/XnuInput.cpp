// SPDX-License-Identifier: BSD-2-Clause-Patent
// XnuInput: a keyboard and an absolute pointer (a tablet) for the hosted macOS, as IOHIDDevices whose reports IOHIDFamily's
// event drivers turn into keyboard and pointer events. It follows DroidVMInput of Droid-VM/macOS-DroidVM
// (overlay/kext/DroidVMInput/DroidVMInput.cpp, BSD-2-Clause-Patent): the same report descriptors (QEMU's USB keyboard and tablet),
// the same evdev-to-report translation. Its events come from the host instead of virtio-input: Linux evdev records
// {u16 type, u16 code, s32 value} that tools/hosted/xnu-input.py appends to xnu.ko's input_in file, read through the hosted
// kernel's upcall (linux64_input_read, bsd/kern/linux64_host.c). Keys (EV_KEY codes below 0x100) go to the keyboard; buttons,
// absolute X/Y (0..32767 across the screen) and the wheel to the tablet; SYN_REPORT ends a frame.
#include <IOKit/IOBufferMemoryDescriptor.h>
#include <IOKit/IOLib.h>
#include <IOKit/IOService.h>
#include <IOKit/hid/IOHIDDevice.h>
#include <kern/thread.h>

extern "C" long linux64_input_read(void *buffer, unsigned long size);

namespace {

enum : uint16_t { evSyn = 0, evKey = 1, evRel = 2, evAbs = 3, synReport = 0 };
enum : uint16_t { absX = 0x00, absY = 0x01, relWheel = 0x08, btnLeft = 0x110, btnRight = 0x111, btnMiddle = 0x112 };
enum class Kind : uint8_t { keyboard, tablet };
constexpr unsigned pollMs = 4;

// usb_kbd_keycode of Linux (drivers/hid/usbhid/usbkbd.c): HID usage -> Linux key code; inverted at start (DroidVMInput's table).
const uint8_t hidToLinux[256] = {
    0,  0,  0,  0, 30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38,
   50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,  2,  3,
    4,  5,  6,  7,  8,  9, 10, 11, 28,  1, 14, 15, 57, 12, 13, 26,
   27, 43, 43, 39, 40, 41, 51, 52, 53, 58, 59, 60, 61, 62, 63, 64,
   65, 66, 67, 68, 87, 88, 99, 70,119,110,102,104,111,107,109,106,
  105,108,103, 69, 98, 55, 74, 78, 96, 79, 80, 81, 75, 76, 77, 71,
   72, 73, 82, 83, 86,127,116,117,183,184,185,186,187,188,189,190,
  191,192,193,194,134,138,130,132,128,129,131,137,133,135,136,113,
  115,114,  0,  0,  0,121,  0, 89, 93,124, 92, 94, 95,  0,  0,  0,
  122,123, 90, 91, 85,  0,  0,  0,  0,  0,  0,  0,111,  0,  0,  0,
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
   29, 42, 56,125, 97, 54,100,126,164,166,165,163,161,115,114,113,
  150,158,159,128,136,177,178,176,142,152,173,140,
};
uint8_t linuxToHid[256];

// QEMU hw/input/hid.c / dev-hid.c report descriptors (as DroidVMInput).
const uint8_t keyboardDescriptor[] = {
  0x05, 0x01, 0x09, 0x06, 0xa1, 0x01,
  0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01, 0x81, 0x02,
  0x95, 0x01, 0x75, 0x08, 0x81, 0x01,
  0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02,
  0x95, 0x01, 0x75, 0x03, 0x91, 0x01,
  0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x26, 0xff, 0x00, 0x05, 0x07, 0x19, 0x00, 0x2a, 0xff, 0x00, 0x81, 0x00,
  0xc0,
};
const uint8_t tabletDescriptor[] = {
  0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00,
  0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02,
  0x95, 0x01, 0x75, 0x05, 0x81, 0x01,
  0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x7f, 0x35, 0x00, 0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x02,
  0x81, 0x02,
  0x05, 0x01, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x35, 0x00, 0x45, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06,
  0xc0, 0xc0,
};

struct Event { uint16_t type, code; int32_t value; };
static_assert(sizeof(Event) == 8, "evdev record");

int8_t clamp8(int32_t v) { return static_cast<int8_t>(v < -127 ? -127 : v > 127 ? 127 : v); }
uint16_t clamp15(int32_t v) { return static_cast<uint16_t>(v < 0 ? 0 : v > 0x7fff ? 0x7fff : v); }

}  // namespace

class XnuHIDDevice : public IOHIDDevice {
  OSDeclareDefaultStructors(XnuHIDDevice)

public:
  Kind fKind = Kind::keyboard;

  IOReturn newReportDescriptor(IOMemoryDescriptor **descriptor) const override;
  OSString *newTransportString() const override { return OSString::withCString("Virtual"); }
  OSString *newManufacturerString() const override { return OSString::withCString("xnulinux"); }
  OSString *newProductString() const override
  {
    return OSString::withCString(fKind == Kind::keyboard ? "xnulinux Keyboard" : "xnulinux Tablet");
  }
  OSNumber *newVendorIDNumber() const override { return OSNumber::withNumber(0x1209, 32); }   // pid.codes' open-source vendor ID
  OSNumber *newProductIDNumber() const override { return OSNumber::withNumber(fKind == Kind::keyboard ? 0x5801 : 0x5802, 32); }
  OSNumber *newLocationIDNumber() const override { return OSNumber::withNumber(fKind == Kind::keyboard ? 0x58550001 : 0x58550002, 32); }
  OSNumber *newPrimaryUsagePageNumber() const override { return OSNumber::withNumber(kHIDPage_GenericDesktop, 32); }
  OSNumber *newPrimaryUsageNumber() const override
  {
    return OSNumber::withNumber(fKind == Kind::keyboard ? kHIDUsage_GD_Keyboard : kHIDUsage_GD_Mouse, 32);
  }
};

OSDefineMetaClassAndStructors(XnuHIDDevice, IOHIDDevice)

IOReturn XnuHIDDevice::newReportDescriptor(IOMemoryDescriptor **descriptor) const
{
  const uint8_t *bytes = fKind == Kind::keyboard ? keyboardDescriptor : tabletDescriptor;
  size_t length = fKind == Kind::keyboard ? sizeof(keyboardDescriptor) : sizeof(tabletDescriptor);
  *descriptor = IOBufferMemoryDescriptor::withBytes(bytes, length, kIODirectionNone);
  return *descriptor ? kIOReturnSuccess : kIOReturnNoMemory;
}

class XnuInput : public IOService {
  OSDeclareDefaultStructors(XnuInput)

  XnuHIDDevice *fKeyboard = nullptr, *fTablet = nullptr;
  uint8_t fModifiers = 0, fKeys[6] = {}, fLastKeyboard[8] = {};
  int32_t fX = 0, fY = 0, fWheel = 0;
  uint8_t fButtons = 0, fLastTablet[6] = {};
  bool fFirstTablet = true;
  Event fEvents[64];

  XnuHIDDevice *makeDevice(Kind kind);
  void event(const Event &e);
  void report();
  static void pollThread(void *self, wait_result_t);

public:
  bool start(IOService *provider) override;
};

OSDefineMetaClassAndStructors(XnuInput, IOService)

XnuHIDDevice *XnuInput::makeDevice(Kind kind)
{
  XnuHIDDevice *device = OSTypeAlloc(XnuHIDDevice);
  if (!device)
    return nullptr;
  device->fKind = kind;
  if (!device->init(nullptr) || !device->attach(this)) {
    device->release();
    return nullptr;
  }
  if (!device->start(this)) {
    device->detach(this);
    device->release();
    return nullptr;
  }
  return device;
}

bool XnuInput::start(IOService *provider)
{
  if (!IOService::start(provider))
    return false;
  for (unsigned usage = 0; usage < 256; usage++)
    if (hidToLinux[usage] && !linuxToHid[hidToLinux[usage]])
      linuxToHid[hidToLinux[usage]] = static_cast<uint8_t>(usage);
  fKeyboard = makeDevice(Kind::keyboard);
  fTablet = makeDevice(Kind::tablet);
  if (!fKeyboard || !fTablet) {
    IOLog("XnuInput: cannot start the HID devices (keyboard %p, tablet %p)\n", fKeyboard, fTablet);
    return false;
  }
  registerService();
  thread_t thread;
  if (kernel_thread_start(pollThread, this, &thread) != KERN_SUCCESS)
    return false;
  thread_deallocate(thread);
  IOLog("XnuInput: keyboard and tablet, events from the host's input_in\n");
  return true;
}

void XnuInput::event(const Event &e)
{
  if (e.type == evSyn) {
    if (e.code == synReport)
      report();
    return;
  }
  if (e.type == evKey && e.code < 0x100) {
    if (e.value == 2)   // autorepeat: the event system repeats on its own
      return;
    uint8_t usage = linuxToHid[e.code];
    if (usage >= 0xe0) {
      uint8_t mask = static_cast<uint8_t>(1 << (usage - 0xe0));
      fModifiers = e.value ? fModifiers | mask : fModifiers & ~mask;
    } else if (usage) {
      for (auto &key : fKeys)
        if (key == usage)
          key = 0;
      for (auto &key : fKeys)
        if (e.value && !key) {
          key = usage;
          break;
        }
    }
  } else if (e.type == evKey) {
    uint8_t mask = e.code == btnLeft ? 1 : e.code == btnRight ? 2 : e.code == btnMiddle ? 4 : 0;
    fButtons = e.value ? fButtons | mask : fButtons & ~mask;
  } else if (e.type == evAbs) {
    if (e.code == absX)
      fX = e.value;
    else if (e.code == absY)
      fY = e.value;
  } else if (e.type == evRel && e.code == relWheel) {
    fWheel += e.value;
  }
}

// At the end of a frame: a report from each device whose state changed.
void XnuInput::report()
{
  uint8_t keyboard[8] = {fModifiers, 0};
  memcpy(keyboard + 2, fKeys, sizeof(fKeys));
  if (memcmp(keyboard, fLastKeyboard, sizeof(keyboard))) {
    memcpy(fLastKeyboard, keyboard, sizeof(keyboard));
    if (IOBufferMemoryDescriptor *r = IOBufferMemoryDescriptor::withBytes(keyboard, sizeof(keyboard), kIODirectionNone)) {
      fKeyboard->handleReport(r);
      r->release();
    }
  }
  uint16_t x = clamp15(fX), y = clamp15(fY);
  int8_t wheel = clamp8(fWheel);
  fWheel = 0;
  uint8_t tablet[6] = {fButtons, static_cast<uint8_t>(x), static_cast<uint8_t>(x >> 8), static_cast<uint8_t>(y),
                       static_cast<uint8_t>(y >> 8), static_cast<uint8_t>(wheel)};
  if (fFirstTablet || wheel || memcmp(tablet, fLastTablet, sizeof(tablet))) {
    fFirstTablet = false;
    memcpy(fLastTablet, tablet, sizeof(tablet));
    if (IOBufferMemoryDescriptor *r = IOBufferMemoryDescriptor::withBytes(tablet, sizeof(tablet), kIODirectionNone)) {
      fTablet->handleReport(r);
      r->release();
    }
  }
}

void XnuInput::pollThread(void *self, wait_result_t)
{
  XnuInput *input = static_cast<XnuInput *>(self);
  for (;;) {
    long bytes = linux64_input_read(input->fEvents, sizeof(input->fEvents));
    for (long i = 0; i + 8 <= bytes; i += 8)
      input->event(input->fEvents[i / 8]);
    if (bytes < static_cast<long>(sizeof(input->fEvents)))
      IOSleep(pollMs);
  }
}

// Module start/stop for the collection's kext entry; the driver starts through IOKit matching.
#include <mach/kmod.h>
extern "C" {
kern_return_t native_kext_start(kmod_info_t *, void *) { return KERN_SUCCESS; }
kern_return_t native_kext_stop(kmod_info_t *, void *) { return KERN_SUCCESS; }
}

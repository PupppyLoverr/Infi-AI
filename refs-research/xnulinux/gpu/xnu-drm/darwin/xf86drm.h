/* SPDX-License-Identifier: MIT */
/*
 * The part of libdrm's xf86drm.h that Mesa's Venus driver (vn_renderer_virtgpu.c) uses, for macOS: libxnudrm implements
 * it over kexts/XnuDrm, which forwards to the Linux host's virtio-gpu render node (gpu/xnu-drm).
 */
#ifndef XNUDRM_XF86DRM_H
#define XNUDRM_XF86DRM_H
#include <stdint.h>
#include <stddef.h>
#include "drm-uapi/drm.h"
#ifdef __cplusplus
extern "C" {
#endif

#define DRM_NODE_PRIMARY 0
#define DRM_NODE_CONTROL 1
#define DRM_NODE_RENDER 2
#define DRM_NODE_MAX 3
#define DRM_BUS_PCI 0
#define DRM_BUS_USB 1
#define DRM_BUS_PLATFORM 2
#define DRM_BUS_HOST1X 3

typedef struct _drmVersion {
	int version_major, version_minor, version_patchlevel;
	int name_len; char *name;
	int date_len; char *date;
	int desc_len; char *desc;
} drmVersion, *drmVersionPtr;

typedef struct _drmPciBusInfo { uint16_t domain; uint8_t bus, dev, func; } drmPciBusInfo, *drmPciBusInfoPtr;
typedef struct _drmPciDeviceInfo {
	uint16_t vendor_id, device_id, subvendor_id, subdevice_id;
	uint8_t revision_id;
} drmPciDeviceInfo, *drmPciDeviceInfoPtr;
typedef struct _drmPlatformBusInfo { char fullname[512]; } drmPlatformBusInfo, *drmPlatformBusInfoPtr;
typedef struct _drmPlatformDeviceInfo { char **compatible; } drmPlatformDeviceInfo, *drmPlatformDeviceInfoPtr;

typedef struct _drmDevice {
	char **nodes;
	int available_nodes;
	int bustype;
	union { drmPciBusInfoPtr pci; drmPlatformBusInfoPtr platform; } businfo;
	union { drmPciDeviceInfoPtr pci; drmPlatformDeviceInfoPtr platform; } deviceinfo;
} drmDevice, *drmDevicePtr;

int drmIoctl(int fd, unsigned long request, void *arg);
int drmGetDevices2(uint32_t flags, drmDevicePtr devices[], int max_devices);
void drmFreeDevices(drmDevicePtr devices[], int count);
drmVersionPtr drmGetVersion(int fd);
void drmFreeVersion(drmVersionPtr v);

#ifdef __cplusplus
}
#endif
#endif

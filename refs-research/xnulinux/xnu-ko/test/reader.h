/* SPDX-License-Identifier: APSL-2.0 */
/*
 * What XNU reads of the boot data xnu.ko hands it, written from XNU's readers (xnu-12377.121.6) and sharing nothing
 * with the module's writers: see reader.c. Nothing of xnu-ko is included here or there.
 */
#pragma once
#include <stdint.h>

/* ---- the flattened device tree (pexpert/pexpert/device_tree.h, pexpert/gen/device_tree.c) ---- */
struct xr_prop { char name[33]; uint32_t len; const unsigned char *val; };
struct xr_node {
	struct xr_node *parent, *child, *next;	/* its first child; its next sibling */
	unsigned nprops, nchildren;
	struct xr_prop *props;
};
/*
 * The whole tree at dt, walked as XNU walks it, and stricter: every node has a property (XNU takes a node of none for
 * the end of the tree), every name is terminated inside its 32 bytes and zero-filled, every value and its padding to 4
 * bytes lies inside the len bytes, the padding is zero, and the tree ends at len exactly. 0 and a line in err otherwise.
 */
struct xr_node *xr_dt_parse(const void *dt, unsigned long len, char *err, unsigned long errlen);
void xr_dt_free(struct xr_node *n);
const struct xr_prop *xr_prop(const struct xr_node *n, const char *name);	/* the first of that name: SecureDTGetProperty */
unsigned xr_prop_count(const struct xr_node *n, const char *name);
const char *xr_name(const struct xr_node *n);					/* its "name", or 0 */
const struct xr_node *xr_lookup(const struct xr_node *root, const char *path);	/* "/chosen/memory-map": SecureDTLookupEntry */
const struct xr_node *xr_find(const struct xr_node *n, const char *prop, const char *value);	/* SecureDTFindEntry */
/* the value is one C string counted with its terminator, as every reader that takes a string from the tree expects */
int xr_is_str(const struct xr_prop *p);
int xr_str_eq(const struct xr_prop *p, const char *s);
uint64_t xr_u64(const struct xr_prop *p, unsigned i);	/* the i-th 64-bit cell of the value */
uint32_t xr_u32(const struct xr_prop *p, unsigned i);

/* ---- boot_args (pexpert/pexpert/arm64/boot.h): its fields' offsets, from that header compiled for arm64 ---- */
enum {
	XR_BA_REVISION = 0, XR_BA_VERSION = 2, XR_BA_VIRTBASE = 8, XR_BA_PHYSBASE = 16, XR_BA_MEMSIZE = 24,
	XR_BA_TOPOFKERNELDATA = 32, XR_BA_V_BASEADDR = 40, XR_BA_V_DISPLAY = 48, XR_BA_V_ROWBYTES = 56, XR_BA_V_WIDTH = 64,
	XR_BA_V_HEIGHT = 72, XR_BA_V_DEPTH = 80, XR_BA_MACHINETYPE = 88, XR_BA_DEVICETREEP = 96, XR_BA_DEVICETREELENGTH = 104,
	XR_BA_COMMANDLINE = 108, XR_BA_BOOTFLAGS = 1136, XR_BA_MEMSIZEACTUAL = 1144, XR_BA_SIZE = 1152,
	XR_BOOT_LINE_LENGTH = 1024,
};
uint64_t xr_ba64(const void *ba, unsigned off);
uint32_t xr_ba32(const void *ba, unsigned off);
uint16_t xr_ba16(const void *ba, unsigned off);

/*
 * A boot-arg as PE_parse_boot_argn finds it on the command line (pexpert/gen/bootargs.c): arguments are separated by
 * spaces and tabs; "-name" is found at its first appearance; "name=value" at its last (a later one replaces an earlier
 * one's value); a bare "name" counts as "1". 1 and the value's text in val (at most max - 1 characters), or 0.
 */
int xr_bootarg(const char *line, const char *name, char *val, unsigned long max);

/* ---- SHA-256 (FIPS 180-4) ---- */
void xr_sha256(const void *data, unsigned long len, unsigned char out[32]);

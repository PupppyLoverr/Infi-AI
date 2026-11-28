/* SPDX-License-Identifier: MIT */
/*
 * The DRM bridge's messages: from a macOS process (libxnudrm, through kexts/XnuDrm and a host call) to xnudrmd, the Linux
 * service that owns the DRM render node. Shared by all three ends; plain C, fixed-size little-endian fields.
 *
 * A message is a header and a payload. XNUDRM_IOCTL carries a Linux DRM ioctl: the payload is the ioctl's argument struct
 * followed by an aux area; each user pointer inside the struct is replaced by an offset into the payload (from the start of
 * the payload), and the reply's payload has the same layout with the outputs filled in.
 */
#ifndef XNUDRM_PROTO_H
#define XNUDRM_PROTO_H
#include <stdint.h>

#define XNUDRM_MAGIC 0x4d52444eu	/* "NDRM" */
#define XNUDRM_VERSION 1
#define XNUDRM_MAX_MSG (8u << 20)
#define XNUDRM_CONNECT_TYPE 0x584e4452u	/* IOServiceOpen type of the bridge's user client: 'XNDR' */
/* status is a Linux errno; the ones a client tests by value (macOS numbers some differently: ETIME is 101 there) */
#define XNUDRM_ETIME 62

enum xnudrm_op {
	XNUDRM_OPEN = 1,	/* -> client (a new DRM file on the render node); arg0 = XNUDRM_VERSION */
	XNUDRM_CLOSE,		/* client: closes the file, its mappings and fences */
	XNUDRM_IOCTL,		/* client, arg0 = Linux ioctl number; payload as above */
	XNUDRM_MAP,		/* client, arg0 = GEM handle, arg1 = size -> arg0 = map id; payload: struct xnudrm_range[] */
	XNUDRM_UNMAP,		/* client, arg0 = map id */
	XNUDRM_FENCE_WAIT,	/* client, arg0 = fence id, arg1 = timeout in ns (0: poll) -> status 0, -XNUDRM_ETIME */
	XNUDRM_FENCE_CLOSE,	/* client, arg0 = fence id */
};

struct xnudrm_hdr {
	uint32_t magic;
	uint32_t op;
	uint32_t client;
	int32_t status;		/* reply: 0 or a negative Linux errno */
	uint64_t arg0, arg1;
	uint32_t payload_len;
	uint32_t pad;
};

/* A physically contiguous piece of a mapping, in order. */
struct xnudrm_range {
	uint64_t pa, len;
};

#endif

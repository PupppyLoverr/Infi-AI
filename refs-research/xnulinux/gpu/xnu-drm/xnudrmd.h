/* SPDX-License-Identifier: GPL-2.0-only */
/* xnudrmd's parts: the service loop (xnudrmd.c) and the virglrenderer backend (xnudrmd_virgl.c). */
#ifndef XNUDRMD_H
#define XNUDRMD_H
#include <stddef.h>
#include <stdint.h>
#include "xnudrm_proto.h"

extern int verbose;

/* Physical ranges of a mapping of this process. */
int xnudrmd_ranges(void *va, size_t len, struct xnudrm_range *rg, uint32_t *nr);

/* Pin a mapping's pages (shared memory only: -EOPNOTSUPP for a device mapping, which has none to pin). */
int xnudrmd_pin(void *va, size_t len);
void xnudrmd_unpin(void *va);

int vg_init(void);
void vg_serve(const struct xnudrm_hdr *h, const uint8_t *payload, struct xnudrm_hdr *rh, uint8_t *rpayload, uint32_t *rlen);

#endif

// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/*
 * ccchacha20poly1305.h: ChaCha20-Poly1305 (RFC 8439). The context is the callers' storage for a provider's state; its
 * layout (ChaCha20's state and buffer, Poly1305's, the lengths and the call state) sets its size.
 */
#ifndef _CORECRYPTO_CCCHACHA20POLY1305_H_
#define _CORECRYPTO_CCCHACHA20POLY1305_H_

#include <corecrypto/cc.h>

#define CCCHACHA20_KEY_NBYTES   32
#define CCCHACHA20_BLOCK_NBYTES 64
#define CCCHACHA20_BLOCK_NBITS  (CCCHACHA20_BLOCK_NBYTES * 8)
#define CCCHACHA20_NONCE_NBYTES 12

typedef struct {
	uint32_t state[16];
	uint8_t buffer[CCCHACHA20_BLOCK_NBYTES];
	size_t leftover;
} ccchacha20_ctx;

#define CCPOLY1305_TAG_NBYTES 16
#define CCPOLY1305_KEY_NBYTES 32

typedef struct {
	uint32_t r0, r1, r2, r3, r4;
	uint32_t s1, s2, s3, s4;
	uint32_t h0, h1, h2, h3, h4;
	uint8_t buf[16];
	size_t buf_used;
	uint8_t key[16];
} ccpoly1305_ctx;

#define CCCHACHA20POLY1305_KEY_NBYTES   (CCCHACHA20_KEY_NBYTES)
#define CCCHACHA20POLY1305_NONCE_NBYTES (CCCHACHA20_NONCE_NBYTES)
#define CCCHACHA20POLY1305_TAG_NBYTES   (CCPOLY1305_TAG_NBYTES)
#define CCCHACHA20POLY1305_TEXT_MAX_NBYTES ((1ULL << 38) - 64ULL)

typedef struct {
	ccchacha20_ctx chacha20_ctx;
	ccpoly1305_ctx poly1305_ctx;
	uint64_t aad_nbytes;
	uint64_t text_nbytes;
	uint8_t state;
} ccchacha20poly1305_ctx;

struct ccchacha20poly1305_info {
};

const struct ccchacha20poly1305_info *ccchacha20poly1305_info(void);
int ccchacha20poly1305_init(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *key);
int ccchacha20poly1305_reset(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx);
int ccchacha20poly1305_setnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx,
    const uint8_t *nonce);
int ccchacha20poly1305_incnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *nonce);
int ccchacha20poly1305_aad(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *aad);
int ccchacha20poly1305_encrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ptext, void *ctext);
int ccchacha20poly1305_finalize(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *tag);
int ccchacha20poly1305_decrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ctext, void *ptext);
int ccchacha20poly1305_verify(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx,
    const uint8_t *tag);

#endif /* _CORECRYPTO_CCCHACHA20POLY1305_H_ */

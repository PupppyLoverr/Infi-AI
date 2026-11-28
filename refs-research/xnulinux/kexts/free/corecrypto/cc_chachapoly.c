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
 * ChaCha20-Poly1305 (RFC 8439): Mbed TLS's, its context in the caller's ccchacha20poly1305_ctx. Mbed TLS fixes the
 * direction when the nonce is set; the interface learns it from the first encrypt or decrypt call, so the nonce starts
 * an encryption and a decryption that comes before any text turns it around (the direction only decides which side of
 * the cipher Poly1305 reads).
 */
#include "cc_internal.h"
#include <mbedtls/chachapoly.h>
#include <mbedtls/platform_util.h>

_Static_assert(sizeof(mbedtls_chachapoly_context) <= sizeof(ccchacha20poly1305_ctx),
    "Mbed TLS's ChaCha20-Poly1305 context must fit the interface's");

#define C(ctx) ((mbedtls_chachapoly_context *)(void *)(ctx))

/* Mbed TLS's states of a message (library/chachapoly.c of the pinned release) */
enum { CCP_INIT = 0, CCP_AAD = 1, CCP_TEXT = 2 };

static const struct ccchacha20poly1305_info xc_ccp_info_value;

const struct ccchacha20poly1305_info *
xc_ccp_info(void)
{
	return &xc_ccp_info_value;
}

int
xc_ccp_init(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *key)
{
	(void)info;
	mbedtls_chachapoly_init(C(ctx));
	return mbedtls_chachapoly_setkey(C(ctx), key) ? CCERR_PARAMETER : 0;
}

/* the key kept, the message dropped: nothing but setnonce goes on from here */
int
xc_ccp_reset(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx)
{
	mbedtls_chachapoly_context *c = C(ctx);

	(void)info;
	mbedtls_platform_zeroize(&c->poly1305_ctx, sizeof(c->poly1305_ctx));
	c->aad_len = 0;
	c->ciphertext_len = 0;
	c->state = CCP_INIT;
	return 0;
}

int
xc_ccp_setnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *nonce)
{
	(void)info;
	return mbedtls_chachapoly_starts(C(ctx), nonce, MBEDTLS_CHACHAPOLY_ENCRYPT) ? CCERR_PARAMETER : 0;
}

/*
 * Not provided: chacha20poly1305_incnonce (libkern/crypto/chacha20poly1305.h) returns CCMODE_NOT_SUPPORTED. Nothing in
 * the kernel calls it; ESP sets each packet's nonce (setnonce).
 */
int
xc_ccp_incnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *nonce)
{
	(void)info;
	(void)ctx;
	(void)nonce;
	return CCMODE_NOT_SUPPORTED;
}

int
xc_ccp_aad(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes, const void *aad)
{
	(void)info;
	return mbedtls_chachapoly_update_aad(C(ctx), aad, nbytes) ? CCERR_CALL_SEQUENCE : 0;
}

static int
crypt(ccchacha20poly1305_ctx *ctx, size_t nbytes, const void *in, void *out, mbedtls_chachapoly_mode_t mode)
{
	mbedtls_chachapoly_context *c = C(ctx);

	if (c->mode != mode) {
		if (c->ciphertext_len != 0) {
			return CCERR_CALL_SEQUENCE;
		}
		c->mode = mode;
	}
	return mbedtls_chachapoly_update(c, nbytes, in, out) ? CCERR_CALL_SEQUENCE : 0;
}

int
xc_ccp_encrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ptext, void *ctext)
{
	(void)info;
	return crypt(ctx, nbytes, ptext, ctext, MBEDTLS_CHACHAPOLY_ENCRYPT);
}

int
xc_ccp_decrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ctext, void *ptext)
{
	(void)info;
	return crypt(ctx, nbytes, ctext, ptext, MBEDTLS_CHACHAPOLY_DECRYPT);
}

/* a message ends once: Mbed TLS would finish a finished one again (state 3) */
static int
finish(ccchacha20poly1305_ctx *ctx, uint8_t tag[CCPOLY1305_TAG_NBYTES])
{
	mbedtls_chachapoly_context *c = C(ctx);

	if (c->state != CCP_AAD && c->state != CCP_TEXT) {	/* a nonce set, the message not ended */
		return CCERR_CALL_SEQUENCE;
	}
	return mbedtls_chachapoly_finish(c, tag) ? CCERR_CALL_SEQUENCE : 0;
}

int
xc_ccp_finalize(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *tag)
{
	(void)info;
	return finish(ctx, tag);
}

/* 0 only when the tag is the message's */
int
xc_ccp_verify(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *tag)
{
	uint8_t t[CCPOLY1305_TAG_NBYTES];
	int rc;

	(void)info;
	if (finish(ctx, t)) {
		return CCERR_CALL_SEQUENCE;
	}
	rc = cc_cmp_safe(sizeof(t), t, tag) ? CCMODE_INTEGRITY_FAILURE : 0;
	mbedtls_platform_zeroize(t, sizeof(t));
	return rc;
}

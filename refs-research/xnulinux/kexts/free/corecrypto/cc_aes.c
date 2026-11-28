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
 * AES in the interface's modes. The block cipher, its key schedule and GCM are Mbed TLS's.
 *
 * ECB, CBC, CTR and XTS keep a compact key (the rounds and round keys of an Mbed TLS context, which fits the kernel's
 * fixed AES contexts: libkern/crypto/aes.h) and run each call on an Mbed TLS context loaded from it. XTS's tweak
 * (IEEE 1619) is the sector IV encrypted with the tweak key, multiplied by x in GF(2^128) after each block.
 */
#include "cc_internal.h"
#include <mbedtls/aes.h>
#include <mbedtls/gcm.h>
#include <mbedtls/platform_util.h>

struct xc_aes_key {
	uint32_t nr;
	uint32_t rk[60];
};

static int
aes_schedule(struct xc_aes_key *k, size_t key_nbytes, const void *key, int enc)
{
	mbedtls_aes_context c;
	int rc;

	if (key_nbytes != 16 && key_nbytes != 24 && key_nbytes != 32) {
		return CCMODE_INVALID_INPUT;
	}
	mbedtls_aes_init(&c);
	rc = enc ? mbedtls_aes_setkey_enc(&c, key, (unsigned)key_nbytes * 8) :
	    mbedtls_aes_setkey_dec(&c, key, (unsigned)key_nbytes * 8);
	if (rc == 0) {
		k->nr = (uint32_t)c.nr;
		memcpy(k->rk, c.buf + c.rk_offset, (k->nr + 1) * 16);
	}
	mbedtls_aes_free(&c);
	mbedtls_platform_zeroize(&c, sizeof(c));
	return rc ? CCMODE_INTERNAL_ERROR : 0;
}

static void
aes_load(mbedtls_aes_context *c, const struct xc_aes_key *k)
{
	c->nr = (int)k->nr;
	c->rk_offset = 0;
	memcpy(c->buf, k->rk, (k->nr + 1) * 16);
}

static void
aes_blocks(const struct xc_aes_key *k, int mode, size_t nblocks, const uint8_t *in, uint8_t *out)
{
	mbedtls_aes_context c;

	aes_load(&c, k);
	for (; nblocks; nblocks--, in += 16, out += 16) {
		mbedtls_aes_crypt_ecb(&c, mode, in, out);
	}
	mbedtls_platform_zeroize(&c, sizeof(c));
}

/* ECB */
static int
ecb_init_enc(const struct ccmode_ecb *m, ccecb_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return aes_schedule((struct xc_aes_key *)ctx, n, key, 1);
}

static int
ecb_init_dec(const struct ccmode_ecb *m, ccecb_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return aes_schedule((struct xc_aes_key *)ctx, n, key, 0);
}

static int
ecb_enc(const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out)
{
	aes_blocks((const struct xc_aes_key *)ctx, MBEDTLS_AES_ENCRYPT, nblocks, in, out);
	return 0;
}

static int
ecb_dec(const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out)
{
	aes_blocks((const struct xc_aes_key *)ctx, MBEDTLS_AES_DECRYPT, nblocks, in, out);
	return 0;
}

static const struct ccmode_ecb aes_ecb_enc = {
	.size = sizeof(struct xc_aes_key), .block_size = 16, .init = ecb_init_enc, .ecb = ecb_enc,
};
static const struct ccmode_ecb aes_ecb_dec = {
	.size = sizeof(struct xc_aes_key), .block_size = 16, .init = ecb_init_dec, .ecb = ecb_dec,
};

/* CBC: the IV is the caller's and is left as the last ciphertext block */
static int
cbc_init_enc(const struct ccmode_cbc *m, cccbc_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return aes_schedule((struct xc_aes_key *)ctx, n, key, 1);
}

static int
cbc_init_dec(const struct ccmode_cbc *m, cccbc_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return aes_schedule((struct xc_aes_key *)ctx, n, key, 0);
}

static int
cbc_run(const cccbc_ctx *ctx, int mode, cccbc_iv *iv, size_t nblocks, const void *in, void *out)
{
	mbedtls_aes_context c;
	int rc;

	if (nblocks == 0) {
		return 0;
	}
	if (nblocks > SIZE_MAX / 16) {
		return CCMODE_INVALID_INPUT;
	}
	aes_load(&c, (const struct xc_aes_key *)ctx);
	rc = mbedtls_aes_crypt_cbc(&c, mode, nblocks * 16, (unsigned char *)iv, in, out);
	mbedtls_platform_zeroize(&c, sizeof(c));
	return rc ? CCMODE_INTERNAL_ERROR : 0;
}

static int
cbc_enc(const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in, void *out)
{
	return cbc_run(ctx, MBEDTLS_AES_ENCRYPT, iv, nblocks, in, out);
}

static int
cbc_dec(const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in, void *out)
{
	return cbc_run(ctx, MBEDTLS_AES_DECRYPT, iv, nblocks, in, out);
}

static const struct ccmode_cbc aes_cbc_enc = {
	.size = sizeof(struct xc_aes_key), .block_size = 16, .init = cbc_init_enc, .cbc = cbc_enc,
};
static const struct ccmode_cbc aes_cbc_dec = {
	.size = sizeof(struct xc_aes_key), .block_size = 16, .init = cbc_init_dec, .cbc = cbc_dec,
};

/* CTR: a 128-bit big-endian counter block; the unused keystream of the last block carries over between calls */
struct xc_ctr {
	struct xc_aes_key key;
	uint8_t counter[16];
	uint8_t stream[16];
	size_t offset;
};

static int
ctr_setctr(const struct ccmode_ctr *m, ccctr_ctx *ctx, const void *ctr)
{
	struct xc_ctr *c = (struct xc_ctr *)ctx;

	(void)m;
	memcpy(c->counter, ctr, 16);
	c->offset = 0;
	return 0;
}

static int
ctr_init(const struct ccmode_ctr *m, ccctr_ctx *ctx, size_t n, const void *key, const void *iv)
{
	int rc = aes_schedule(&((struct xc_ctr *)ctx)->key, n, key, 1);
	return rc ? rc : ctr_setctr(m, ctx, iv);
}

static int
ctr_crypt(ccctr_ctx *ctx, size_t nbytes, const void *in, void *out)
{
	struct xc_ctr *c = (struct xc_ctr *)ctx;
	mbedtls_aes_context a;
	int rc;

	aes_load(&a, &c->key);
	rc = mbedtls_aes_crypt_ctr(&a, nbytes, &c->offset, c->counter, c->stream, in, out);
	mbedtls_platform_zeroize(&a, sizeof(a));
	return rc ? CCMODE_INTERNAL_ERROR : 0;
}

static const struct ccmode_ctr aes_ctr = {
	.size = sizeof(struct xc_ctr), .block_size = 1, .ecb_block_size = 16, .init = ctr_init, .setctr = ctr_setctr,
	.ctr = ctr_crypt,
};

/* XTS */
struct xc_xts {
	struct xc_aes_key data;
	struct xc_aes_key tweak;
};

static int
xts_init_dir(ccxts_ctx *ctx, size_t n, const void *dk, const void *tk, int enc)
{
	struct xc_xts *x = (struct xc_xts *)ctx;
	int rc;

	if (n != 16 && n != 32) {	/* XTS-AES-128 and -256 only */
		return CCERR_PARAMETER;
	}
	if (cc_cmp_safe(n, dk, tk) == 0) {
		return CCERR_XTS_KEYS_EQUAL;
	}
	rc = aes_schedule(&x->data, n, dk, enc);
	return rc ? rc : aes_schedule(&x->tweak, n, tk, 1);
}

static int
xts_init_enc(const struct ccmode_xts *m, ccxts_ctx *ctx, size_t n, const void *dk, const void *tk)
{
	(void)m;
	return xts_init_dir(ctx, n, dk, tk, 1);
}

static int
xts_init_dec(const struct ccmode_xts *m, ccxts_ctx *ctx, size_t n, const void *dk, const void *tk)
{
	(void)m;
	return xts_init_dir(ctx, n, dk, tk, 0);
}

static void
xts_key_sched_enc(const struct ccmode_xts *m, ccxts_ctx *ctx, size_t n, const void *dk, const void *tk)
{
	(void)xts_init_enc(m, ctx, n, dk, tk);
}

static void
xts_key_sched_dec(const struct ccmode_xts *m, ccxts_ctx *ctx, size_t n, const void *dk, const void *tk)
{
	(void)xts_init_dec(m, ctx, n, dk, tk);
}

static int
xts_set_tweak(const ccxts_ctx *ctx, ccxts_tweak *tweak, const void *iv)
{
	aes_blocks(&((const struct xc_xts *)ctx)->tweak, MBEDTLS_AES_ENCRYPT, 1, iv, (uint8_t *)tweak);
	return 0;
}

/* T = T * x in GF(2^128), T little-endian (IEEE 1619) */
static void
xts_next(uint8_t t[16])
{
	uint8_t carry = t[15] >> 7;

	for (int i = 15; i > 0; i--) {
		t[i] = (uint8_t)((t[i] << 1) | (t[i - 1] >> 7));
	}
	t[0] = (uint8_t)((t[0] << 1) ^ (0x87 & -carry));
}

static void *
xts_run(const ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in, void *out, int mode)
{
	const struct xc_xts *x = (const struct xc_xts *)ctx;
	uint8_t *t = (uint8_t *)tweak, b[16];
	const uint8_t *p = in;
	uint8_t *q = out;
	mbedtls_aes_context c;

	aes_load(&c, &x->data);
	for (; nblocks; nblocks--, p += 16, q += 16) {
		for (int i = 0; i < 16; i++) {
			b[i] = p[i] ^ t[i];
		}
		mbedtls_aes_crypt_ecb(&c, mode, b, b);
		for (int i = 0; i < 16; i++) {
			q[i] = b[i] ^ t[i];
		}
		xts_next(t);
	}
	mbedtls_platform_zeroize(&c, sizeof(c));
	mbedtls_platform_zeroize(b, sizeof(b));
	return tweak;
}

static void *
xts_enc(const ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in, void *out)
{
	return xts_run(ctx, tweak, nblocks, in, out, MBEDTLS_AES_ENCRYPT);
}

static void *
xts_dec(const ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in, void *out)
{
	return xts_run(ctx, tweak, nblocks, in, out, MBEDTLS_AES_DECRYPT);
}

static const struct ccmode_xts aes_xts_enc = {
	.size = sizeof(struct xc_xts), .tweak_size = 16, .block_size = 16, .init = xts_init_enc,
	.key_sched = xts_key_sched_enc, .set_tweak = xts_set_tweak, .xts = xts_enc,
};
static const struct ccmode_xts aes_xts_dec = {
	.size = sizeof(struct xc_xts), .tweak_size = 16, .block_size = 16, .init = xts_init_dec,
	.key_sched = xts_key_sched_dec, .set_tweak = xts_set_tweak, .xts = xts_dec,
};

/*
 * GCM: Mbed TLS's context, the IV (kept for inc_iv) and where in the call order the context is. A decryptor's finalize
 * compares the tag given with the one computed.
 */
enum { GCM_KEY, GCM_IV, GCM_AAD, GCM_TEXT, GCM_DONE };

struct xc_gcm {
	mbedtls_gcm_context g;
	uint8_t iv[CCGCM_IV_NBYTES];
	uint8_t state;
	uint8_t with_iv;
	uint8_t decrypt;
};

static int
gcm_init_dir(ccgcm_ctx *ctx, size_t n, const void *key, int decrypt)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;

	if (n != 16 && n != 24 && n != 32) {
		return CCMODE_INVALID_INPUT;
	}
	memset(c, 0, sizeof(*c));
	mbedtls_gcm_init(&c->g);
	if (mbedtls_gcm_setkey(&c->g, MBEDTLS_CIPHER_ID_AES, key, (unsigned)n * 8) != 0) {
		return CCMODE_INTERNAL_ERROR;
	}
	c->decrypt = (uint8_t)decrypt;
	c->state = GCM_IV;
	return 0;
}

static int
gcm_init_enc(const struct ccmode_gcm *m, ccgcm_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return gcm_init_dir(ctx, n, key, 0);
}

static int
gcm_init_dec(const struct ccmode_gcm *m, ccgcm_ctx *ctx, size_t n, const void *key)
{
	(void)m;
	return gcm_init_dir(ctx, n, key, 1);
}

static int
gcm_start(struct xc_gcm *c, size_t n, const void *iv)
{
	if (mbedtls_gcm_starts(&c->g, c->decrypt ? MBEDTLS_GCM_DECRYPT : MBEDTLS_GCM_ENCRYPT, iv, n) != 0) {
		return CCMODE_INVALID_INPUT;
	}
	c->state = GCM_AAD;
	return 0;
}

static int
gcm_set_iv(ccgcm_ctx *ctx, size_t n, const void *iv)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;

	if (c->state != GCM_IV || n == 0) {
		return c->state != GCM_IV ? CCMODE_INVALID_CALL_SEQUENCE : CCMODE_INVALID_INPUT;
	}
	if (n == CCGCM_IV_NBYTES) {
		memcpy(c->iv, iv, n);
	}
	return gcm_start(c, n, iv);
}

static int
gcm_aad(ccgcm_ctx *ctx, size_t n, const void *in)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;

	if (c->state != GCM_AAD) {
		return CCMODE_INVALID_CALL_SEQUENCE;
	}
	if (n == 0) {
		return 0;
	}
	return mbedtls_gcm_update_ad(&c->g, in, n) ? CCMODE_INVALID_INPUT : 0;
}

static int
gcm_crypt(ccgcm_ctx *ctx, size_t n, const void *in, void *out)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;
	size_t done;

	if (c->state != GCM_AAD && c->state != GCM_TEXT) {
		return CCMODE_INVALID_CALL_SEQUENCE;
	}
	c->state = GCM_TEXT;
	if (n == 0) {
		return 0;
	}
	if (mbedtls_gcm_update(&c->g, in, n, out, n, &done) != 0 || done != n) {
		return CCMODE_INVALID_INPUT;
	}
	return 0;
}

static int
gcm_finalize(ccgcm_ctx *ctx, size_t tag_nbytes, void *tag)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;
	uint8_t t[16];
	size_t none;
	int rc = 0;

	if (c->state != GCM_AAD && c->state != GCM_TEXT) {
		return CCMODE_INVALID_CALL_SEQUENCE;
	}
	/* the tag lengths of NIST SP 800-38D (5.2.1.2): 12 to 16 bytes, and 4 or 8 */
	if (tag_nbytes != 4 && tag_nbytes != 8 && (tag_nbytes < 12 || tag_nbytes > sizeof(t))) {
		return CCMODE_INVALID_INPUT;
	}
	if (mbedtls_gcm_finish(&c->g, NULL, 0, &none, t, sizeof(t)) != 0) {
		return CCMODE_INTERNAL_ERROR;
	}
	if (c->decrypt) {
		rc = cc_cmp_safe(tag_nbytes, t, tag) ? CCMODE_INTEGRITY_FAILURE : 0;
	} else {
		memcpy(tag, t, tag_nbytes);
	}
	mbedtls_platform_zeroize(t, sizeof(t));
	c->state = GCM_DONE;
	return rc;
}

/* back to before the IV, the key and the IV kept */
static int
gcm_reset(ccgcm_ctx *ctx)
{
	((struct xc_gcm *)ctx)->state = GCM_IV;
	return 0;
}

static const struct ccmode_gcm aes_gcm_enc = {
	.size = sizeof(struct xc_gcm), .encdec = CCMODE_GCM_ENCRYPTOR, .block_size = 1, .init = gcm_init_enc,
	.set_iv = gcm_set_iv, .gmac = gcm_aad, .gcm = gcm_crypt, .finalize = gcm_finalize, .reset = gcm_reset,
};
static const struct ccmode_gcm aes_gcm_dec = {
	.size = sizeof(struct xc_gcm), .encdec = CCMODE_GCM_DECRYPTOR, .block_size = 1, .init = gcm_init_dec,
	.set_iv = gcm_set_iv, .gmac = gcm_aad, .gcm = gcm_crypt, .finalize = gcm_finalize, .reset = gcm_reset,
};

int
xc_gcm_init_with_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key, const void *iv)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;
	int rc = mode->init(mode, ctx, key_nbytes, key);

	if (rc == 0) {
		rc = gcm_set_iv(ctx, CCGCM_IV_NBYTES, iv);
	}
	if (rc == 0) {
		c->with_iv = 1;
	}
	return rc;
}

/* the next IV: the last 8 bytes of the IV counted up (big-endian); after a reset, on a context made with its IV */
int
xc_gcm_inc_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, void *iv)
{
	struct xc_gcm *c = (struct xc_gcm *)ctx;

	(void)mode;
	if (c->state != GCM_IV || !c->with_iv) {
		return CCMODE_INVALID_CALL_SEQUENCE;
	}
	for (int i = CCGCM_IV_NBYTES - 1; i >= CCGCM_IV_NBYTES - 8; i--) {
		if (++c->iv[i] != 0) {
			break;
		}
	}
	memcpy(iv, c->iv, CCGCM_IV_NBYTES);
	return gcm_start(c, CCGCM_IV_NBYTES, c->iv);
}

const struct ccmode_ecb *xc_aes_ecb_encrypt(void) { return &aes_ecb_enc; }
const struct ccmode_ecb *xc_aes_ecb_decrypt(void) { return &aes_ecb_dec; }
const struct ccmode_cbc *xc_aes_cbc_encrypt(void) { return &aes_cbc_enc; }
const struct ccmode_cbc *xc_aes_cbc_decrypt(void) { return &aes_cbc_dec; }
const struct ccmode_ctr *xc_aes_ctr(void) { return &aes_ctr; }
const struct ccmode_xts *xc_aes_xts_encrypt(void) { return &aes_xts_enc; }
const struct ccmode_xts *xc_aes_xts_decrypt(void) { return &aes_xts_dec; }
const struct ccmode_gcm *xc_aes_gcm_encrypt(void) { return &aes_gcm_enc; }
const struct ccmode_gcm *xc_aes_gcm_decrypt(void) { return &aes_gcm_dec; }

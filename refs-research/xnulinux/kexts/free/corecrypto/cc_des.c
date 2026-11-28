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
 * DES and triple DES (Mbed TLS's), for Kerberos and the other protocols that still name them, and CBC with ciphertext
 * stealing (CS3: the last two blocks always swapped, RFC 3962) over any CBC mode.
 */
#include "cc_internal.h"
#include <mbedtls/des.h>
#include <mbedtls/platform_util.h>

/* DES */
static int
des_init_enc(size_t n, void *ctx, const void *key)
{
	return n == 8 && mbedtls_des_setkey_enc(ctx, key) == 0 ? 0 : CCMODE_INVALID_INPUT;
}

static int
des_init_dec(size_t n, void *ctx, const void *key)
{
	return n == 8 && mbedtls_des_setkey_dec(ctx, key) == 0 ? 0 : CCMODE_INVALID_INPUT;
}

static int des_ecb_init_enc(const struct ccmode_ecb *m, ccecb_ctx *c, size_t n, const void *k) { (void)m; return des_init_enc(n, c, k); }
static int des_ecb_init_dec(const struct ccmode_ecb *m, ccecb_ctx *c, size_t n, const void *k) { (void)m; return des_init_dec(n, c, k); }
static int des_cbc_init_enc(const struct ccmode_cbc *m, cccbc_ctx *c, size_t n, const void *k) { (void)m; return des_init_enc(n, c, k); }
static int des_cbc_init_dec(const struct ccmode_cbc *m, cccbc_ctx *c, size_t n, const void *k) { (void)m; return des_init_dec(n, c, k); }

static int
des_ecb(const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out)
{
	const uint8_t *p = in;
	uint8_t *q = out;

	for (; nblocks; nblocks--, p += 8, q += 8) {
		mbedtls_des_crypt_ecb((mbedtls_des_context *)ctx, p, q);
	}
	return 0;
}

static int
des_cbc(const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in, void *out, int mode)
{
	if (nblocks == 0) {
		return 0;
	}
	if (nblocks > SIZE_MAX / 8) {
		return CCMODE_INVALID_INPUT;
	}
	return mbedtls_des_crypt_cbc((mbedtls_des_context *)ctx, mode, nblocks * 8, (unsigned char *)iv, in, out) ?
	       CCMODE_INTERNAL_ERROR : 0;
}

static int des_cbc_enc(const cccbc_ctx *c, cccbc_iv *iv, size_t n, const void *in, void *out) { return des_cbc(c, iv, n, in, out, MBEDTLS_DES_ENCRYPT); }
static int des_cbc_dec(const cccbc_ctx *c, cccbc_iv *iv, size_t n, const void *in, void *out) { return des_cbc(c, iv, n, in, out, MBEDTLS_DES_DECRYPT); }

/* triple DES: two keys (16 bytes) or three (24) */
static int
des3_init(size_t n, void *ctx, const void *key, int enc)
{
	int rc;

	if (n == 16) {
		rc = enc ? mbedtls_des3_set2key_enc(ctx, key) : mbedtls_des3_set2key_dec(ctx, key);
	} else if (n == 24) {
		rc = enc ? mbedtls_des3_set3key_enc(ctx, key) : mbedtls_des3_set3key_dec(ctx, key);
	} else {
		return CCMODE_INVALID_INPUT;
	}
	return rc ? CCMODE_INVALID_INPUT : 0;
}

static int des3_ecb_init_enc(const struct ccmode_ecb *m, ccecb_ctx *c, size_t n, const void *k) { (void)m; return des3_init(n, c, k, 1); }
static int des3_ecb_init_dec(const struct ccmode_ecb *m, ccecb_ctx *c, size_t n, const void *k) { (void)m; return des3_init(n, c, k, 0); }
static int des3_cbc_init_enc(const struct ccmode_cbc *m, cccbc_ctx *c, size_t n, const void *k) { (void)m; return des3_init(n, c, k, 1); }
static int des3_cbc_init_dec(const struct ccmode_cbc *m, cccbc_ctx *c, size_t n, const void *k) { (void)m; return des3_init(n, c, k, 0); }

static int
des3_ecb(const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out)
{
	const uint8_t *p = in;
	uint8_t *q = out;

	for (; nblocks; nblocks--, p += 8, q += 8) {
		mbedtls_des3_crypt_ecb((mbedtls_des3_context *)ctx, p, q);
	}
	return 0;
}

static int
des3_cbc(const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in, void *out, int mode)
{
	if (nblocks == 0) {
		return 0;
	}
	if (nblocks > SIZE_MAX / 8) {
		return CCMODE_INVALID_INPUT;
	}
	return mbedtls_des3_crypt_cbc((mbedtls_des3_context *)ctx, mode, nblocks * 8, (unsigned char *)iv, in, out) ?
	       CCMODE_INTERNAL_ERROR : 0;
}

static int des3_cbc_enc(const cccbc_ctx *c, cccbc_iv *iv, size_t n, const void *in, void *out) { return des3_cbc(c, iv, n, in, out, MBEDTLS_DES_ENCRYPT); }
static int des3_cbc_dec(const cccbc_ctx *c, cccbc_iv *iv, size_t n, const void *in, void *out) { return des3_cbc(c, iv, n, in, out, MBEDTLS_DES_DECRYPT); }

#define DES_ECB(name, sz, i, f) static const struct ccmode_ecb name = { .size = sz, .block_size = 8, .init = i, .ecb = f }
#define DES_CBC(name, sz, i, f) static const struct ccmode_cbc name = { .size = sz, .block_size = 8, .init = i, .cbc = f }
DES_ECB(des_ecb_enc_mode, sizeof(mbedtls_des_context), des_ecb_init_enc, des_ecb);
DES_ECB(des_ecb_dec_mode, sizeof(mbedtls_des_context), des_ecb_init_dec, des_ecb);
DES_CBC(des_cbc_enc_mode, sizeof(mbedtls_des_context), des_cbc_init_enc, des_cbc_enc);
DES_CBC(des_cbc_dec_mode, sizeof(mbedtls_des_context), des_cbc_init_dec, des_cbc_dec);
DES_ECB(des3_ecb_enc_mode, sizeof(mbedtls_des3_context), des3_ecb_init_enc, des3_ecb);
DES_ECB(des3_ecb_dec_mode, sizeof(mbedtls_des3_context), des3_ecb_init_dec, des3_ecb);
DES_CBC(des3_cbc_enc_mode, sizeof(mbedtls_des3_context), des3_cbc_init_enc, des3_cbc_enc);
DES_CBC(des3_cbc_dec_mode, sizeof(mbedtls_des3_context), des3_cbc_init_dec, des3_cbc_dec);

const struct ccmode_ecb *xc_des_ecb_encrypt(void) { return &des_ecb_enc_mode; }
const struct ccmode_ecb *xc_des_ecb_decrypt(void) { return &des_ecb_dec_mode; }
const struct ccmode_cbc *xc_des_cbc_encrypt(void) { return &des_cbc_enc_mode; }
const struct ccmode_cbc *xc_des_cbc_decrypt(void) { return &des_cbc_dec_mode; }
const struct ccmode_ecb *xc_des3_ecb_encrypt(void) { return &des3_ecb_enc_mode; }
const struct ccmode_ecb *xc_des3_ecb_decrypt(void) { return &des3_ecb_dec_mode; }
const struct ccmode_cbc *xc_des3_cbc_encrypt(void) { return &des3_cbc_enc_mode; }
const struct ccmode_cbc *xc_des3_cbc_decrypt(void) { return &des3_cbc_dec_mode; }

/* nonzero when any 8-byte key in key is weak or semi-weak */
int
xc_des_key_is_weak(void *key, unsigned long length)
{
	const uint8_t *k = key;
	unsigned char b[MBEDTLS_DES_KEY_SIZE];
	int weak = 0;

	/* the table holds the keys with odd parity, and the parity bits are not part of the key */
	for (unsigned long i = 0; !weak && i + 8 <= length; i += 8) {
		memcpy(b, k + i, sizeof(b));
		mbedtls_des_key_set_parity(b);
		weak = mbedtls_des_key_check_weak(b);
	}
	mbedtls_platform_zeroize(b, sizeof(b));
	return weak != 0;
}

void
xc_des_key_set_odd_parity(void *key, unsigned long length)
{
	uint8_t *k = key;

	for (unsigned long i = 0; i + 8 <= length; i += 8) {
		mbedtls_des_key_set_parity(k + i);
	}
}

/*
 * CBC-CS3. Up to the last two blocks it is CBC; the last full plaintext block and the (zero-padded) partial one are
 * CBC-encrypted to Ca, Cb and written as Cb, then Ca cut to the partial length. One block is plain CBC. The IV is left
 * as CBC would leave it (Cb).
 */
#define CTS_MAX_BLOCK 16

size_t
xc_cts3_encrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in, void *out)
{
	size_t b = cbc->block_size, r, m;
	const uint8_t *p = in;
	uint8_t *q = out, last2[2 * CTS_MAX_BLOCK] = {0}, c[2 * CTS_MAX_BLOCK];

	if (b > CTS_MAX_BLOCK || nbytes < b) {
		return 0;
	}
	if (nbytes == b) {
		return cbc->cbc(ctx, iv, 1, p, q) ? 0 : nbytes;
	}
	r = nbytes % b ? nbytes % b : b;
	m = (nbytes - r) / b;	/* full blocks, the last of them in the swapped pair */
	memcpy(last2, p + (m - 1) * b, b + r);
	if ((m > 1 && cbc->cbc(ctx, iv, m - 1, p, q)) || cbc->cbc(ctx, iv, 2, last2, c)) {
		return 0;
	}
	memcpy(q + (m - 1) * b, c + b, b);
	memcpy(q + m * b, c, r);
	mbedtls_platform_zeroize(last2, sizeof(last2));
	return nbytes;
}

size_t
xc_cts3_decrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in, void *out)
{
	size_t b = cbc->block_size, r, m;
	const uint8_t *p = in;
	uint8_t *q = out, cb[CTS_MAX_BLOCK], ca[CTS_MAX_BLOCK], z[CTS_MAX_BLOCK], pl[CTS_MAX_BLOCK];
	uint8_t zero_iv[CTS_MAX_BLOCK] CC_ALIGNED(16);

	if (b > CTS_MAX_BLOCK || nbytes < b) {
		return 0;
	}
	if (nbytes == b) {
		return cbc->cbc(ctx, iv, 1, p, q) ? 0 : nbytes;
	}
	r = nbytes % b ? nbytes % b : b;
	m = (nbytes - r) / b;
	memcpy(cb, p + (m - 1) * b, b);		/* CBC's last block */
	memcpy(ca, p + m * b, r);		/* CBC's second-last block, cut */
	if (m > 1 && cbc->cbc(ctx, iv, m - 1, p, q)) {
		return 0;
	}
	/* Dec(Cb) = padded last plaintext ^ Ca: its tail is Ca's missing tail, its head the last plaintext */
	memset(zero_iv, 0, sizeof(zero_iv));
	if (cbc->cbc(ctx, (cccbc_iv *)zero_iv, 1, cb, z)) {
		return 0;
	}
	for (size_t i = 0; i < r; i++) {
		pl[i] = z[i] ^ ca[i];
	}
	memcpy(ca + r, z + r, b - r);
	if (cbc->cbc(ctx, iv, 1, ca, q + (m - 1) * b)) {
		return 0;
	}
	memcpy(q + m * b, pl, r);
	memcpy(iv, cb, b);
	mbedtls_platform_zeroize(z, sizeof(z));
	mbedtls_platform_zeroize(pl, sizeof(pl));
	return nbytes;
}

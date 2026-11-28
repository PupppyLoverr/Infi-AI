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
 * Digests and HMAC. A digest's state in the interface's context is Mbed TLS's state array as it is; compressing blocks
 * and finishing load it into an Mbed TLS context and run Mbed TLS's own block function and padding.
 */
#include "cc_internal.h"
#include <mbedtls/md5.h>
#include <mbedtls/sha1.h>
#include <mbedtls/sha256.h>
#include <mbedtls/sha512.h>
#include <mbedtls/platform_util.h>

static uint64_t
total_bytes(const struct ccdigest_info *di, ccdigest_ctx_t ctx)
{
	return ccdigest_nbits(di, ctx) / 8 + ccdigest_num(di, ctx);
}

/* MD5 */
static uint32_t md5_iv[4];

static void
md5_compress(ccdigest_state_t state, size_t nblocks, const void *data)
{
	mbedtls_md5_context c;
	const uint8_t *p = data;

	memcpy(c.state, state, sizeof(c.state));
	for (; nblocks; nblocks--, p += 64) {
		mbedtls_internal_md5_process(&c, p);
	}
	memcpy(state, c.state, sizeof(c.state));
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
md5_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	mbedtls_md5_context c;
	uint64_t n = total_bytes(di, ctx);

	memcpy(c.state, ccdigest_state(di, ctx), sizeof(c.state));
	c.total[0] = (uint32_t)n;
	c.total[1] = (uint32_t)(n >> 32);
	memcpy(c.buffer, ccdigest_data(di, ctx), ccdigest_num(di, ctx));
	mbedtls_md5_finish(&c, out);
	mbedtls_platform_zeroize(&c, sizeof(c));
}

/* SHA-1 */
static uint32_t sha1_iv[5];

static void
sha1_compress(ccdigest_state_t state, size_t nblocks, const void *data)
{
	mbedtls_sha1_context c;
	const uint8_t *p = data;

	memcpy(c.state, state, sizeof(c.state));
	for (; nblocks; nblocks--, p += 64) {
		mbedtls_internal_sha1_process(&c, p);
	}
	memcpy(state, c.state, sizeof(c.state));
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
sha1_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	mbedtls_sha1_context c;
	uint64_t n = total_bytes(di, ctx);

	memcpy(c.state, ccdigest_state(di, ctx), sizeof(c.state));
	c.total[0] = (uint32_t)n;
	c.total[1] = (uint32_t)(n >> 32);
	memcpy(c.buffer, ccdigest_data(di, ctx), ccdigest_num(di, ctx));
	mbedtls_sha1_finish(&c, out);
	mbedtls_platform_zeroize(&c, sizeof(c));
}

/* SHA-224 and SHA-256: one block function, the output differs */
static uint32_t sha224_iv[8], sha256_iv[8];

static void
sha256_compress(ccdigest_state_t state, size_t nblocks, const void *data)
{
	mbedtls_sha256_context c;
	const uint8_t *p = data;

	memcpy(c.state, state, sizeof(c.state));
	for (; nblocks; nblocks--, p += 64) {
		mbedtls_internal_sha256_process(&c, p);
	}
	memcpy(state, c.state, sizeof(c.state));
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
sha256_final_is224(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out, int is224)
{
	mbedtls_sha256_context c;
	uint64_t n = total_bytes(di, ctx);

	memcpy(c.state, ccdigest_state(di, ctx), sizeof(c.state));
	c.total[0] = (uint32_t)n;
	c.total[1] = (uint32_t)(n >> 32);
	c.is224 = is224;
	memcpy(c.buffer, ccdigest_data(di, ctx), ccdigest_num(di, ctx));
	mbedtls_sha256_finish(&c, out);
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
sha224_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	sha256_final_is224(di, ctx, out, 1);
}

static void
sha256_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	sha256_final_is224(di, ctx, out, 0);
}

/* SHA-384 and SHA-512 */
static uint64_t sha384_iv[8], sha512_iv[8];

static void
sha512_compress(ccdigest_state_t state, size_t nblocks, const void *data)
{
	mbedtls_sha512_context c;
	const uint8_t *p = data;

	memcpy(c.state, state, sizeof(c.state));
	for (; nblocks; nblocks--, p += 128) {
		mbedtls_internal_sha512_process(&c, p);
	}
	memcpy(state, c.state, sizeof(c.state));
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
sha512_final_is384(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out, int is384)
{
	mbedtls_sha512_context c;

	memcpy(c.state, ccdigest_state(di, ctx), sizeof(c.state));
	c.total[0] = total_bytes(di, ctx);
	c.total[1] = 0;
	c.is384 = is384;
	memcpy(c.buffer, ccdigest_data(di, ctx), ccdigest_num(di, ctx));
	mbedtls_sha512_finish(&c, out);
	mbedtls_platform_zeroize(&c, sizeof(c));
}

static void
sha384_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	sha512_final_is384(di, ctx, out, 1);
}

static void
sha512_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *out)
{
	sha512_final_is384(di, ctx, out, 0);
}

#define DI(name, out, state, block, oid_der, oid_len, iv, comp, fin) \
	static const struct ccdigest_info name = { \
		.output_size = out, .state_size = state, .block_size = block, .oid_size = oid_len, .oid = oid_der, \
		.initial_state = iv, .compress = comp, .final = fin, \
	}

DI(md5_di, 16, 16, 64, CC_DIGEST_OID_MD5, 10, md5_iv, md5_compress, md5_final);
DI(sha1_di, 20, 20, 64, CC_DIGEST_OID_SHA1, 7, sha1_iv, sha1_compress, sha1_final);
DI(sha224_di, 28, 32, 64, CC_DIGEST_OID_SHA224, 11, sha224_iv, sha256_compress, sha224_final);
DI(sha256_di, 32, 32, 64, CC_DIGEST_OID_SHA256, 11, sha256_iv, sha256_compress, sha256_final);
DI(sha384_di, 48, 64, 128, CC_DIGEST_OID_SHA384, 11, sha384_iv, sha512_compress, sha384_final);
DI(sha512_di, 64, 64, 128, CC_DIGEST_OID_SHA512, 11, sha512_iv, sha512_compress, sha512_final);

const struct ccdigest_info *xc_md5_di(void) { return &md5_di; }
const struct ccdigest_info *xc_sha1_di(void) { return &sha1_di; }
const struct ccdigest_info *xc_sha224_di(void) { return &sha224_di; }
const struct ccdigest_info *xc_sha256_di(void) { return &sha256_di; }
const struct ccdigest_info *xc_sha384_di(void) { return &sha384_di; }
const struct ccdigest_info *xc_sha512_di(void) { return &sha512_di; }

/* the initial states, as Mbed TLS starts each hash (before anything is registered) */
void
xc_digest_setup(void)
{
	mbedtls_md5_context m;
	mbedtls_sha1_context s1;
	mbedtls_sha256_context s2;
	mbedtls_sha512_context s5;

	mbedtls_md5_init(&m);
	mbedtls_md5_starts(&m);
	memcpy(md5_iv, m.state, sizeof(md5_iv));
	mbedtls_sha1_init(&s1);
	mbedtls_sha1_starts(&s1);
	memcpy(sha1_iv, s1.state, sizeof(sha1_iv));
	mbedtls_sha256_init(&s2);
	mbedtls_sha256_starts(&s2, 1);
	memcpy(sha224_iv, s2.state, sizeof(sha224_iv));
	mbedtls_sha256_starts(&s2, 0);
	memcpy(sha256_iv, s2.state, sizeof(sha256_iv));
	mbedtls_sha512_init(&s5);
	mbedtls_sha512_starts(&s5, 1);
	memcpy(sha384_iv, s5.state, sizeof(sha384_iv));
	mbedtls_sha512_starts(&s5, 0);
	memcpy(sha512_iv, s5.state, sizeof(sha512_iv));
}

/*
 * The generic digest calls. The context's bit count is what has been compressed; the pending bytes (fewer than a block)
 * wait in its buffer until a block fills or the digest finishes.
 */
void
xc_digest_init(const struct ccdigest_info *di, ccdigest_ctx_t ctx)
{
	memcpy(ccdigest_state(di, ctx), di->initial_state, di->state_size);
	ccdigest_nbits(di, ctx) = 0;
	memset(ccdigest_data(di, ctx), 0, di->block_size);	/* MD5Init and SHA1Init copy the whole block out */
	ccdigest_num(di, ctx) = 0;
}

void
xc_digest_update(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned long len, const void *data)
{
	const uint8_t *p = data;
	size_t bs = di->block_size;
	unsigned int num = ccdigest_num(di, ctx);

	if (num >= bs) {
		xc_panic("corecrypto: digest context corrupt (%u pending bytes)", num);
	}
	if (num > 0) {
		size_t take = bs - num < len ? bs - num : len;
		memcpy(ccdigest_data(di, ctx) + num, p, take);
		num += take;
		p += take;
		len -= take;
		if (num == bs) {
			di->compress(ccdigest_state(di, ctx), 1, ccdigest_data(di, ctx));
			ccdigest_nbits(di, ctx) += bs * 8;
			num = 0;
		}
	}
	if (len >= bs) {
		size_t n = len / bs;
		di->compress(ccdigest_state(di, ctx), n, p);
		ccdigest_nbits(di, ctx) += (uint64_t)n * bs * 8;
		p += n * bs;
		len -= n * bs;
	}
	if (len > 0) {
		memcpy(ccdigest_data(di, ctx), p, len);
		num = (unsigned int)len;
	}
	ccdigest_num(di, ctx) = num;
}

void
xc_digest_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, void *digest)
{
	di->final(di, ctx, digest);
}

void
xc_digest(const struct ccdigest_info *di, unsigned long len, const void *data, void *digest)
{
	ccdigest_di_decl(di, ctx);
	xc_digest_init(di, ctx);
	xc_digest_update(di, ctx, len, data);
	di->final(di, ctx, digest);
	ccdigest_di_clear(di, ctx);
}

/*
 * HMAC (RFC 2104) over any of the digests: the inner hash runs in the context's digest part from the start; the
 * outer hash's state after its key block waits in the context's ostate.
 */
#define XC_MAX_BLOCK  128
#define XC_MAX_OUTPUT 64

void
xc_hmac_init(const struct ccdigest_info *di, cchmac_ctx_t hc, unsigned long key_len, const void *key)
{
	uint8_t k[XC_MAX_BLOCK] = {0};
	size_t bs = di->block_size;

	if (bs > XC_MAX_BLOCK || di->output_size > XC_MAX_OUTPUT) {
		xc_panic("corecrypto: HMAC over a digest with a %zu-byte block", bs);
	}
	if (key_len > bs) {
		xc_digest(di, key_len, key, k);
	} else if (key_len > 0) {
		memcpy(k, key, key_len);
	}
	for (size_t i = 0; i < bs; i++) {
		k[i] ^= 0x5c;
	}
	memcpy(cchmac_ostate(di, hc), di->initial_state, di->state_size);
	di->compress(cchmac_ostate(di, hc), 1, k);
	for (size_t i = 0; i < bs; i++) {
		k[i] ^= 0x5c ^ 0x36;
	}
	xc_digest_init(di, cchmac_digest_ctx(di, hc));
	xc_digest_update(di, cchmac_digest_ctx(di, hc), bs, k);
	mbedtls_platform_zeroize(k, sizeof(k));
}

void
xc_hmac_update(const struct ccdigest_info *di, cchmac_ctx_t hc, unsigned long data_len, const void *data)
{
	xc_digest_update(di, cchmac_digest_ctx(di, hc), data_len, data);
}

void
xc_hmac_final(const struct ccdigest_info *di, cchmac_ctx_t hc, unsigned char *mac)
{
	uint8_t inner[XC_MAX_OUTPUT];
	ccdigest_ctx_t ctx = cchmac_digest_ctx(di, hc);

	di->final(di, ctx, inner);
	memcpy(ccdigest_state(di, ctx), cchmac_ostate(di, hc), di->state_size);
	ccdigest_nbits(di, ctx) = di->block_size * 8;
	ccdigest_num(di, ctx) = 0;
	xc_digest_update(di, ctx, di->output_size, inner);
	di->final(di, ctx, mac);
	mbedtls_platform_zeroize(inner, sizeof(inner));
}

void
xc_hmac(const struct ccdigest_info *di, unsigned long key_len, const void *key, unsigned long data_len,
    const void *data, unsigned char *mac)
{
	cchmac_di_decl(di, hc);
	xc_hmac_init(di, hc, key_len, key);
	xc_hmac_update(di, hc, data_len, data);
	xc_hmac_final(di, hc, mac);
	cchmac_di_clear(di, hc);
}

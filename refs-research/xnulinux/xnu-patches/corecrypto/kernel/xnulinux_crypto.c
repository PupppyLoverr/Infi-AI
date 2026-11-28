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
 * The kernel side of the corecrypto interface: the calls XNU's own code makes (bsd/, libkern/, osfmk/) that go to the
 * registered crypto provider (register_crypto_functions(): the corecrypto kext). The kernel has no cryptographic
 * algorithm of its own: digests and HMAC are the provider's (g_crypto_funcs), modes go through the mode's table, and
 * early random numbers come from the host (osfmk/prng/prng_random.c).
 */
#include <corecrypto/cc.h>
#include <corecrypto/ccdigest.h>
#include <corecrypto/cchmac.h>
#include <corecrypto/ccmode.h>
#include <corecrypto/ccsha1.h>
#include <corecrypto/ccsha2.h>
#include <corecrypto/ccmd5.h>
#include <libkern/crypto/register_crypto.h>
#include <libkern/crypto/crypto_internal.h>
#include <kern/debug.h>

static crypto_functions_t
provider(void)
{
	crypto_functions_t f = g_crypto_funcs;

	if (__improbable(f == NULL)) {
		panic("corecrypto: used before a crypto provider registered (register_crypto_functions)");
	}
	return f;
}

void
cc_clear(size_t len, void *dst)
{
	memset(dst, 0, len);
	/* the stores happen: the compiler must assume the memory is read */
	__asm__ __volatile__ ("" : : "r"(dst) : "memory");
}

int
cc_cmp_safe(size_t num, const void *ptr1, const void *ptr2)
{
	const volatile uint8_t *a = ptr1, *b = ptr2;
	uint8_t d = 0;

	if (num == 0) {
		return 1;
	}
	for (size_t i = 0; i < num; i++) {
		d |= a[i] ^ b[i];
	}
	return d != 0;
}

/* digests and HMAC: the provider's */

const struct ccdigest_info *
ccmd5_di(void)
{
	return provider()->ccmd5_di;
}

const struct ccdigest_info *
ccsha1_di(void)
{
	return provider()->ccsha1_di;
}

const struct ccdigest_info *
ccsha256_di(void)
{
	return provider()->ccsha256_di;
}

const struct ccdigest_info *
ccsha384_di(void)
{
	return provider()->ccsha384_di;
}

const struct ccdigest_info *
ccsha512_di(void)
{
	return provider()->ccsha512_di;
}

void
ccdigest_init(const struct ccdigest_info *di, ccdigest_ctx_t ctx)
{
	provider()->ccdigest_init_fn(di, ctx);
}

void
ccdigest_update(const struct ccdigest_info *di, ccdigest_ctx_t ctx, size_t len, const void *data)
{
	provider()->ccdigest_update_fn(di, ctx, len, data);
}

void
ccdigest(const struct ccdigest_info *di, size_t len, const void *data, void *digest)
{
	provider()->ccdigest_fn(di, len, data, digest);
}

void
cchmac_init(const struct ccdigest_info *di, cchmac_ctx_t ctx, size_t key_len, const void *key)
{
	provider()->cchmac_init_fn(di, ctx, key_len, key);
}

void
cchmac_update(const struct ccdigest_info *di, cchmac_ctx_t ctx, size_t data_len, const void *data)
{
	provider()->cchmac_update_fn(di, ctx, data_len, data);
}

void
cchmac_final(const struct ccdigest_info *di, cchmac_ctx_t ctx, unsigned char *mac)
{
	provider()->cchmac_final_fn(di, ctx, mac);
}

void
cchmac(const struct ccdigest_info *di, size_t key_len, const void *key, size_t data_len, const void *data,
    unsigned char *mac)
{
	provider()->cchmac_fn(di, key_len, key, data_len, data, mac);
}

/* modes: through the mode's own table */

size_t
ccecb_context_size(const struct ccmode_ecb *mode)
{
	return mode->size;
}

size_t
ccecb_block_size(const struct ccmode_ecb *mode)
{
	return mode->block_size;
}

int
ccecb_init(const struct ccmode_ecb *mode, ccecb_ctx *ctx, size_t key_len, const void *key)
{
	return mode->init(mode, ctx, key_len, key);
}

int
ccecb_update(const struct ccmode_ecb *mode, const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out)
{
	return mode->ecb(ctx, nblocks, in, out);
}

size_t
cccbc_context_size(const struct ccmode_cbc *mode)
{
	return mode->size;
}

size_t
cccbc_block_size(const struct ccmode_cbc *mode)
{
	return mode->block_size;
}

int
cccbc_init(const struct ccmode_cbc *mode, cccbc_ctx *ctx, size_t key_len, const void *key)
{
	return mode->init(mode, ctx, key_len, key);
}

int
cccbc_copy_iv(cccbc_iv *iv_ctx, const void *iv, size_t len)
{
	memcpy(iv_ctx, iv, len);
	return 0;
}

int
cccbc_clear_iv(cccbc_iv *iv_ctx, size_t len)
{
	cc_clear(len, iv_ctx);
	return 0;
}

/* a NULL iv is all zeroes */
int
cccbc_set_iv(const struct ccmode_cbc *mode, cccbc_iv *iv_ctx, const void *iv)
{
	if (iv != NULL) {
		memcpy(iv_ctx, iv, mode->block_size);
	} else {
		cc_clear(mode->block_size, iv_ctx);
	}
	return 0;
}

int
cccbc_update(const struct ccmode_cbc *mode, const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in,
    void *out)
{
	return mode->cbc(ctx, iv, nblocks, in, out);
}

size_t
ccctr_context_size(const struct ccmode_ctr *mode)
{
	return mode->size;
}

size_t
ccctr_block_size(const struct ccmode_ctr *mode)
{
	return mode->block_size;
}

int
ccctr_init(const struct ccmode_ctr *mode, ccctr_ctx *ctx, size_t key_len, const void *key, const void *iv)
{
	return mode->init(mode, ctx, key_len, key, iv);
}

int
ccctr_update(const struct ccmode_ctr *mode, ccctr_ctx *ctx, size_t nbytes, const void *in, void *out)
{
	return mode->ctr(ctx, nbytes, in, out);
}

size_t
ccxts_context_size(const struct ccmode_xts *mode)
{
	return mode->size;
}

size_t
ccxts_block_size(const struct ccmode_xts *mode)
{
	return mode->block_size;
}

int
ccxts_init(const struct ccmode_xts *mode, ccxts_ctx *ctx, size_t key_nbytes, const void *data_key, const void *tweak_key)
{
	return mode->init(mode, ctx, key_nbytes, data_key, tweak_key);
}

int
ccxts_set_tweak(const struct ccmode_xts *mode, ccxts_ctx *ctx, ccxts_tweak *tweak, const void *iv)
{
	return mode->set_tweak(ctx, tweak, iv);
}

void *
ccxts_update(const struct ccmode_xts *mode, ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in,
    void *out)
{
	return mode->xts(ctx, tweak, nblocks, in, out);
}

size_t
ccgcm_context_size(const struct ccmode_gcm *mode)
{
	return mode->size;
}

size_t
ccgcm_block_size(const struct ccmode_gcm *mode)
{
	return mode->block_size;
}

int
ccgcm_init(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key)
{
	return mode->init(mode, ctx, key_nbytes, key);
}

/* these two depend on the provider's context layout: the provider's own */
int
ccgcm_init_with_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key, const void *iv)
{
	return provider()->ccgcm_init_with_iv_fn(mode, ctx, key_nbytes, key, iv);
}

int
ccgcm_inc_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, void *iv)
{
	return provider()->ccgcm_inc_iv_fn(mode, ctx, iv);
}

int
ccgcm_set_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t iv_nbytes, const void *iv)
{
	return mode->set_iv(ctx, iv_nbytes, iv);
}

int
ccgcm_aad(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t nbytes, const void *additional_data)
{
	return mode->gmac(ctx, nbytes, additional_data);
}

int
ccgcm_update(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t nbytes, const void *in, void *out)
{
	return mode->gcm(ctx, nbytes, in, out);
}

int
ccgcm_finalize(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t tag_nbytes, void *tag)
{
	return mode->finalize(ctx, tag_nbytes, tag);
}

int
ccgcm_reset(const struct ccmode_gcm *mode, ccgcm_ctx *ctx)
{
	return mode->reset(ctx);
}

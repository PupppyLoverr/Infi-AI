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
 * The kext: its self tests, then its functions registered with the kernel (register_crypto_functions) and the kernel's
 * random generator registered as the host's (register_and_init_prng).
 */
#include "cc_internal.h"
#include <libkern/crypto/register_crypto.h>
#include <prng/random.h>
#include <mach/kmod.h>
#include <libkern/libkern.h>

kern_return_t corecrypto_start(kmod_info_t *ki, void *d);
kern_return_t corecrypto_stop(kmod_info_t *ki, void *d);

static const struct ccchacha20poly1305_fns chachapoly_fns = {
	.info = xc_ccp_info, .init = xc_ccp_init, .reset = xc_ccp_reset, .setnonce = xc_ccp_setnonce,
	.incnonce = xc_ccp_incnonce, .aad = xc_ccp_aad, .encrypt = xc_ccp_encrypt, .finalize = xc_ccp_finalize,
	.decrypt = xc_ccp_decrypt, .verify = xc_ccp_verify,
};

/* the crypto_digest_* and crypto_hmac_* calls: the ccdigest and cchmac contexts of the algorithm's digest */
static const struct ccdigest_info *
alg_di(crypto_digest_alg_t alg)
{
	switch (alg) {
	case CRYPTO_DIGEST_ALG_MD5: return xc_md5_di();
	case CRYPTO_DIGEST_ALG_SHA1: return xc_sha1_di();
	case CRYPTO_DIGEST_ALG_SHA256: return xc_sha256_di();
	case CRYPTO_DIGEST_ALG_SHA384: return xc_sha384_di();
	case CRYPTO_DIGEST_ALG_SHA512: return xc_sha512_di();
	default: xc_panic("corecrypto: unknown digest algorithm %u", (unsigned)alg);
	}
}

static void
check_ctx(size_t want, size_t have)
{
	if (want != have) {
		xc_panic("corecrypto: context of %zu bytes where %zu are needed", have, want);
	}
}

static void
check_out(const struct ccdigest_info *di, size_t size)
{
	if (size > di->output_size) {
		xc_panic("corecrypto: %zu bytes asked of a %zu-byte digest", size, di->output_size);
	}
}

static size_t
digest_ctx_size(crypto_digest_alg_t alg)
{
	return ccdigest_di_size(alg_di(alg));
}

static void
digest_init(crypto_digest_alg_t alg, void *ctx, size_t ctx_size)
{
	check_ctx(digest_ctx_size(alg), ctx_size);
	xc_digest_init(alg_di(alg), ctx);
}

static void
digest_update(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, const void *data, size_t data_size)
{
	check_ctx(digest_ctx_size(alg), ctx_size);
	xc_digest_update(alg_di(alg), ctx, data_size, data);
}

static void
digest_final(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, void *digest, size_t digest_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];

	check_ctx(digest_ctx_size(alg), ctx_size);
	check_out(di, digest_size);
	xc_digest_final(di, ctx, out);
	memcpy(digest, out, digest_size);
	cc_clear(sizeof(out), out);
}

static void
digest_oneshot(crypto_digest_alg_t alg, const void *data, size_t data_size, void *digest, size_t digest_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];

	check_out(di, digest_size);
	xc_digest(di, data_size, data, out);
	memcpy(digest, out, digest_size);
	cc_clear(sizeof(out), out);
}

static size_t
hmac_ctx_size(crypto_digest_alg_t alg)
{
	return cchmac_di_size(alg_di(alg));
}

static void
hmac_init(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, const void *key, size_t key_size)
{
	check_ctx(hmac_ctx_size(alg), ctx_size);
	xc_hmac_init(alg_di(alg), ctx, key_size, key);
}

static void
hmac_update(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, const void *data, size_t data_size)
{
	check_ctx(hmac_ctx_size(alg), ctx_size);
	xc_hmac_update(alg_di(alg), ctx, data_size, data);
}

static void
hmac_final_generate(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, void *tag, size_t tag_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];

	check_ctx(hmac_ctx_size(alg), ctx_size);
	check_out(di, tag_size);
	xc_hmac_final(di, ctx, out);
	memcpy(tag, out, tag_size);
	cc_clear(sizeof(out), out);
}

static bool
hmac_final_verify(crypto_digest_alg_t alg, void *ctx, size_t ctx_size, const void *tag, size_t tag_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];
	bool ok;

	check_ctx(hmac_ctx_size(alg), ctx_size);
	check_out(di, tag_size);
	xc_hmac_final(di, ctx, out);
	ok = cc_cmp_safe(tag_size, tag, out) == 0;
	cc_clear(sizeof(out), out);
	return ok;
}

static void
hmac_generate(crypto_digest_alg_t alg, const void *key, size_t key_size, const void *data, size_t data_size, void *tag,
    size_t tag_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];

	check_out(di, tag_size);
	xc_hmac(di, key_size, key, data_size, data, out);
	memcpy(tag, out, tag_size);
	cc_clear(sizeof(out), out);
}

static bool
hmac_verify(crypto_digest_alg_t alg, const void *key, size_t key_size, const void *data, size_t data_size,
    const void *tag, size_t tag_size)
{
	const struct ccdigest_info *di = alg_di(alg);
	uint8_t out[64];
	bool ok;

	check_out(di, tag_size);
	xc_hmac(di, key_size, key, data_size, data, out);
	ok = cc_cmp_safe(tag_size, tag, out) == 0;
	cc_clear(sizeof(out), out);
	return ok;
}

/* random numbers: a context is kmem's generator (the only kind crypto_random_* is given); none, the host's */
static struct ccrng_state *
random_rng(crypto_random_ctx_t ctx)
{
	return ctx != NULL ? (struct ccrng_state *)ctx : xc_rng(NULL);
}

static void
random_generate(crypto_random_ctx_t ctx, void *random, size_t random_size)
{
	(void)ccrng_generate(random_rng(ctx), random_size, random);
}

static void
random_uniform(crypto_random_ctx_t ctx, uint64_t bound, uint64_t *random)
{
	if (xc_rng_uniform(random_rng(ctx), bound, random) != 0) {
		xc_panic("corecrypto: crypto_random_uniform with bound 0");
	}
}

static size_t
random_kmem_ctx_size(void)
{
	return xc_kmem_rng_size();
}

static void
random_kmem_init(crypto_random_ctx_t ctx)
{
	xc_kmem_rng_init(ctx);
}

static struct crypto_functions functions = {
	.ccdigest_init_fn = xc_digest_init,
	.ccdigest_update_fn = xc_digest_update,
	.ccdigest_final_fn = xc_digest_final,
	.ccdigest_fn = xc_digest,
	.cchmac_init_fn = xc_hmac_init,
	.cchmac_update_fn = xc_hmac_update,
	.cchmac_final_fn = xc_hmac_final,
	.cchmac_fn = xc_hmac,
	.ccgcm_init_with_iv_fn = xc_gcm_init_with_iv,
	.ccgcm_inc_iv_fn = xc_gcm_inc_iv,
	.ccchacha20poly1305_fns = &chachapoly_fns,
	.ccdes_key_is_weak_fn = xc_des_key_is_weak,
	.ccdes_key_set_odd_parity_fn = xc_des_key_set_odd_parity,
	.ccpad_cts3_encrypt_fn = xc_cts3_encrypt,
	.ccpad_cts3_decrypt_fn = xc_cts3_decrypt,
	.ccrng_fn = xc_rng,
	.ccrsa_make_pub_fn = xc_rsa_make_pub,
	.ccrsa_verify_pkcs1v15_fn = xc_rsa_verify_pkcs1v15,
	.random_generate_fn = random_generate,
	.random_uniform_fn = random_uniform,
	.random_kmem_ctx_size_fn = random_kmem_ctx_size,
	.random_kmem_init_fn = random_kmem_init,
	.digest_ctx_size_fn = digest_ctx_size,
	.digest_init_fn = digest_init,
	.digest_update_fn = digest_update,
	.digest_final_fn = digest_final,
	.digest_fn = digest_oneshot,
	.hmac_ctx_size_fn = hmac_ctx_size,
	.hmac_init_fn = hmac_init,
	.hmac_update_fn = hmac_update,
	.hmac_final_generate_fn = hmac_final_generate,
	.hmac_final_verify_fn = hmac_final_verify,
	.hmac_generate_fn = hmac_generate,
	.hmac_verify_fn = hmac_verify,
};

/* the kernel PRNG: the host's generator; nothing to seed, nothing per CPU */
struct cckprng_ctx {
	int unused;
};
static struct cckprng_ctx kprng_ctx;

static void
kprng_init(struct cckprng_ctx *ctx, size_t seed_nbytes, const void *seed, size_t nonce_nbytes, const void *nonce,
    cckprng_getentropy getentropy, void *arg)
{
	(void)ctx, (void)seed_nbytes, (void)seed, (void)nonce_nbytes, (void)nonce, (void)getentropy, (void)arg;
}

static void
kprng_init_with_getentropy(struct cckprng_ctx *ctx, unsigned max_ngens, size_t seed_nbytes, const void *seed,
    size_t nonce_nbytes, const void *nonce, cckprng_getentropy getentropy, void *arg)
{
	(void)max_ngens;
	kprng_init(ctx, seed_nbytes, seed, nonce_nbytes, nonce, getentropy, arg);
}

static void
kprng_initgen(struct cckprng_ctx *ctx, unsigned gen_idx)
{
	(void)ctx, (void)gen_idx;
}

/* seeds the kernel offers (/dev/random writes among them): the host's generator has its own entropy */
static void
kprng_reseed(struct cckprng_ctx *ctx, size_t nbytes, const void *seed)
{
	(void)ctx, (void)nbytes, (void)seed;
}

static void
kprng_refresh(struct cckprng_ctx *ctx)
{
	(void)ctx;
}

static void
kprng_generate(struct cckprng_ctx *ctx, unsigned gen_idx, size_t nbytes, void *out)
{
	(void)ctx, (void)gen_idx;
	xc_random(out, nbytes);
}

static const struct cckprng_funcs kprng_funcs = {
	.init = kprng_init,
	.initgen = kprng_initgen,
	.reseed = kprng_reseed,
	.refresh = kprng_refresh,
	.generate = kprng_generate,
	.init_with_getentropy = kprng_init_with_getentropy,
};

static void
register_modes(void)
{
	functions.ccmd5_di = xc_md5_di();
	functions.ccsha1_di = xc_sha1_di();
	functions.ccsha256_di = xc_sha256_di();
	functions.ccsha384_di = xc_sha384_di();
	functions.ccsha512_di = xc_sha512_di();
	functions.ccaes_ecb_encrypt = xc_aes_ecb_encrypt();
	functions.ccaes_ecb_decrypt = xc_aes_ecb_decrypt();
	functions.ccaes_cbc_encrypt = xc_aes_cbc_encrypt();
	functions.ccaes_cbc_decrypt = xc_aes_cbc_decrypt();
	functions.ccaes_ctr_crypt = xc_aes_ctr();
	functions.ccaes_xts_encrypt = xc_aes_xts_encrypt();
	functions.ccaes_xts_decrypt = xc_aes_xts_decrypt();
	functions.ccaes_gcm_encrypt = xc_aes_gcm_encrypt();
	functions.ccaes_gcm_decrypt = xc_aes_gcm_decrypt();
	functions.ccdes_ecb_encrypt = xc_des_ecb_encrypt();
	functions.ccdes_ecb_decrypt = xc_des_ecb_decrypt();
	functions.ccdes_cbc_encrypt = xc_des_cbc_encrypt();
	functions.ccdes_cbc_decrypt = xc_des_cbc_decrypt();
	functions.cctdes_ecb_encrypt = xc_des3_ecb_encrypt();
	functions.cctdes_ecb_decrypt = xc_des3_ecb_decrypt();
	functions.cctdes_cbc_encrypt = xc_des3_cbc_encrypt();
	functions.cctdes_cbc_decrypt = xc_des3_cbc_decrypt();
}

kern_return_t
corecrypto_start(kmod_info_t *ki, void *d)
{
	int failed;

	(void)ki, (void)d;
	xc_digest_setup();
	failed = xc_selftest();
	if (failed) {
		panic("corecrypto: self test %d failed", failed);
	}
	register_modes();
	if (register_crypto_functions(&functions) != 0) {
		panic("corecrypto: another crypto provider is registered");
	}
	register_and_init_prng(&kprng_ctx, &kprng_funcs);
	printf("corecrypto: xnulinux's (Mbed TLS), self tests passed; random numbers from the host\n");
	return KERN_SUCCESS;
}

kern_return_t
corecrypto_stop(kmod_info_t *ki, void *d)
{
	(void)ki, (void)d;
	return KERN_FAILURE;	/* the kernel's crypto cannot go away */
}

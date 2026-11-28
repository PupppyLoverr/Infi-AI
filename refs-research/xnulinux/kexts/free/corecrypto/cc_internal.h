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
 * xnulinux's corecrypto kext: the corecrypto interface's provider for the kernel collection without Apple's kexts
 * (kc-free.bin). Every primitive is Mbed TLS's; this kext adapts it to the interface's tables and context layouts. Random
 * numbers are the host's (Linux's get_random_bytes): the kext has no generator of its own.
 *
 * The same sources build for user space (test/): CC_TEST_USERSPACE replaces the kernel's services.
 */
#ifndef XNULINUX_CC_INTERNAL_H
#define XNULINUX_CC_INTERNAL_H

#define MBEDTLS_ALLOW_PRIVATE_ACCESS
#ifndef MBEDTLS_CONFIG_FILE
#define MBEDTLS_CONFIG_FILE "mbedtls_config_xnulinux.h"
#endif

#include <corecrypto/cc.h>
#include <corecrypto/ccdigest.h>
#include <corecrypto/cchmac.h>
#include <corecrypto/ccmode.h>
#include <corecrypto/ccaes.h>
#include <corecrypto/ccdes.h>
#include <corecrypto/ccpad.h>
#include <corecrypto/ccsha1.h>
#include <corecrypto/ccsha2.h>
#include <corecrypto/ccmd5.h>
#include <corecrypto/ccrng.h>
#include <corecrypto/ccrsa.h>
#include <corecrypto/ccchacha20poly1305.h>
#include <corecrypto/cckprng.h>

#if CC_TEST_USERSPACE
#include <stdio.h>
#include <stdlib.h>
#define xc_panic(...) do { fprintf(stderr, __VA_ARGS__); fputc('\n', stderr); abort(); } while (0)
void xc_random(void *buf, size_t nbytes);
/* the tests' allocator: each free must name the size its allocation had, as IOFreeData does */
void xc_test_alloc(void *p, size_t bytes);
void xc_test_free(void *p, size_t bytes);
#else
#include <kern/debug.h>
#include <linux64/linux64.h>
#define xc_panic(...) panic(__VA_ARGS__)
/* the host's generator (Linux's get_random_bytes), through LINUX64_CALL_RANDOM */
static inline void
xc_random(void *buf, size_t nbytes)
{
	if (!linux64_hook) {
		panic("corecrypto: no host to take random numbers from");
	}
	linux64_random(buf, nbytes);
}
#endif

/* every function of the interface this kext provides, by name, for the registration (cc_kext.c) and the tests */
const struct ccdigest_info *xc_md5_di(void);
const struct ccdigest_info *xc_sha1_di(void);
const struct ccdigest_info *xc_sha224_di(void);
const struct ccdigest_info *xc_sha256_di(void);
const struct ccdigest_info *xc_sha384_di(void);
const struct ccdigest_info *xc_sha512_di(void);
void xc_digest_setup(void);
void xc_digest_init(const struct ccdigest_info *di, ccdigest_ctx_t ctx);
void xc_digest_update(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned long len, const void *data);
void xc_digest_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, void *digest);
void xc_digest(const struct ccdigest_info *di, unsigned long len, const void *data, void *digest);
void xc_hmac_init(const struct ccdigest_info *di, cchmac_ctx_t ctx, unsigned long key_len, const void *key);
void xc_hmac_update(const struct ccdigest_info *di, cchmac_ctx_t ctx, unsigned long data_len, const void *data);
void xc_hmac_final(const struct ccdigest_info *di, cchmac_ctx_t ctx, unsigned char *mac);
void xc_hmac(const struct ccdigest_info *di, unsigned long key_len, const void *key, unsigned long data_len,
    const void *data, unsigned char *mac);

const struct ccmode_ecb *xc_aes_ecb_encrypt(void);
const struct ccmode_ecb *xc_aes_ecb_decrypt(void);
const struct ccmode_cbc *xc_aes_cbc_encrypt(void);
const struct ccmode_cbc *xc_aes_cbc_decrypt(void);
const struct ccmode_ctr *xc_aes_ctr(void);
const struct ccmode_xts *xc_aes_xts_encrypt(void);
const struct ccmode_xts *xc_aes_xts_decrypt(void);
const struct ccmode_gcm *xc_aes_gcm_encrypt(void);
const struct ccmode_gcm *xc_aes_gcm_decrypt(void);
int xc_gcm_init_with_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key,
    const void *iv);
int xc_gcm_inc_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, void *iv);

const struct ccmode_ecb *xc_des_ecb_encrypt(void);
const struct ccmode_ecb *xc_des_ecb_decrypt(void);
const struct ccmode_cbc *xc_des_cbc_encrypt(void);
const struct ccmode_cbc *xc_des_cbc_decrypt(void);
const struct ccmode_ecb *xc_des3_ecb_encrypt(void);
const struct ccmode_ecb *xc_des3_ecb_decrypt(void);
const struct ccmode_cbc *xc_des3_cbc_encrypt(void);
const struct ccmode_cbc *xc_des3_cbc_decrypt(void);
int xc_des_key_is_weak(void *key, unsigned long length);
void xc_des_key_set_odd_parity(void *key, unsigned long length);
size_t xc_cts3_encrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in,
    void *out);
size_t xc_cts3_decrypt(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, cccbc_iv *iv, size_t nbytes, const void *in,
    void *out);

const struct ccchacha20poly1305_info *xc_ccp_info(void);
int xc_ccp_init(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *key);
int xc_ccp_reset(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx);
int xc_ccp_setnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *nonce);
int xc_ccp_incnonce(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *nonce);
int xc_ccp_aad(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes, const void *aad);
int xc_ccp_encrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ptext, void *ctext);
int xc_ccp_finalize(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, uint8_t *tag);
int xc_ccp_decrypt(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, size_t nbytes,
    const void *ctext, void *ptext);
int xc_ccp_verify(const struct ccchacha20poly1305_info *info, ccchacha20poly1305_ctx *ctx, const uint8_t *tag);

int xc_rsa_make_pub(ccrsa_pub_ctx_t pubk, size_t exp_nbytes, const uint8_t *exp, size_t mod_nbytes,
    const uint8_t *mod);
int xc_rsa_verify_pkcs1v15(ccrsa_pub_ctx_t key, const uint8_t *oid, size_t digest_len, const uint8_t *digest,
    size_t sig_len, const uint8_t *sig, bool *valid);

struct ccrng_state *xc_rng(int *error);
int xc_rng_uniform(struct ccrng_state *rng, uint64_t bound, uint64_t *rand);
/* kmem's generator, a ccrng that takes no lock once initialized (crypto_random_kmem_*) */
size_t xc_kmem_rng_size(void);
void xc_kmem_rng_init(void *ctx);

/* known-answer tests of every primitive, through the interface (cc_selftest.c): 0 when all pass */
int xc_selftest(void);

#endif /* XNULINUX_CC_INTERNAL_H */

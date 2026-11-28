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
 * ccmode_impl.h: a block cipher mode as a table of its context sizes and functions. A provider fills these; callers
 * allocate a context of `size` bytes (ccmode.h's *_ctx_decl) and go through the table, or the wrappers in ccmode.h.
 */
#ifndef _CORECRYPTO_CCMODE_IMPL_H_
#define _CORECRYPTO_CCMODE_IMPL_H_

#include <corecrypto/cc.h>

#define CCMODE_MAX_BLOCK_SIZE 16

/* ECB: a key schedule, then blocks */
cc_aligned_struct(16) ccecb_ctx;
struct ccmode_ecb {
	size_t size;
	size_t block_size;
	int (*CC_SPTR(ccmode_ecb, init))(const struct ccmode_ecb *ecb, ccecb_ctx *ctx, size_t key_nbytes, const void *key);
	int (*CC_SPTR(ccmode_ecb, ecb))(const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out);
	void (*CC_SPTR(ccmode_ecb, roundkey))(const ccecb_ctx *ctx, unsigned r, void *key);
	cc_impl_t impl;
};

/* CBC: the IV lives with the caller and is updated by each call */
cc_aligned_struct(16) cccbc_ctx;
cc_aligned_struct(16) cccbc_iv;
#define CCCBC_MAX_CTX_SIZE 512
struct ccmode_cbc {
	size_t size;
	size_t block_size;
	int (*CC_SPTR(ccmode_cbc, init))(const struct ccmode_cbc *cbc, cccbc_ctx *ctx, size_t key_len, const void *key);
	int (*CC_SPTR(ccmode_cbc, cbc))(const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in, void *out);
	const void *custom;
};

/* CTR: a byte stream; block_size is 1, ecb_block_size the cipher's */
cc_aligned_struct(16) ccctr_ctx;
struct ccmode_ctr {
	size_t size;
	size_t block_size;
	size_t ecb_block_size;
	int (*CC_SPTR(ccmode_ctr, init))(const struct ccmode_ctr *mode, ccctr_ctx *ctx, size_t key_len, const void *key,
	    const void *iv);
	int (*CC_SPTR(ccmode_ctr, setctr))(const struct ccmode_ctr *mode, ccctr_ctx *ctx, const void *ctr);
	int (*CC_SPTR(ccmode_ctr, ctr))(ccctr_ctx *ctx, size_t nbytes, const void *in, void *out);
	const void *custom;
};

/* XTS: a data key and a tweak key; the tweak (set from the sector's IV) lives with the caller */
cc_aligned_struct(16) ccxts_ctx;
cc_aligned_struct(16) ccxts_tweak;
struct ccmode_xts {
	size_t size;
	size_t tweak_size;
	size_t block_size;
	int (*CC_SPTR(ccmode_xts, init))(const struct ccmode_xts *xts, ccxts_ctx *ctx, size_t key_nbytes,
	    const void *data_key, const void *tweak_key);
	void (*CC_SPTR(ccmode_xts, key_sched))(const struct ccmode_xts *xts, ccxts_ctx *ctx, size_t key_nbytes,
	    const void *data_key, const void *tweak_key);
	int (*CC_SPTR(ccmode_xts, set_tweak))(const ccxts_ctx *ctx, ccxts_tweak *tweak, const void *iv);
	void *(*CC_SPTR(ccmode_xts, xts))(const ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in,
	    void *out);
	const void *custom;
	const void *custom1;
	cc_impl_t impl;
};

/* GCM: one context per key, in one direction (encdec) */
cc_aligned_struct(16) ccgcm_ctx;
#define CCMODE_GCM_DECRYPTOR 78647
#define CCMODE_GCM_ENCRYPTOR 4073947
struct ccmode_gcm {
	size_t size;
	int encdec;
	size_t block_size;
	int (*CC_SPTR(ccmode_gcm, init))(const struct ccmode_gcm *gcm, ccgcm_ctx *ctx, size_t key_nbytes, const void *key);
	int (*CC_SPTR(ccmode_gcm, set_iv))(ccgcm_ctx *ctx, size_t iv_nbytes, const void *iv);
	int (*CC_SPTR(ccmode_gcm, gmac))(ccgcm_ctx *ctx, size_t nbytes, const void *in);
	int (*CC_SPTR(ccmode_gcm, gcm))(ccgcm_ctx *ctx, size_t nbytes, const void *in, void *out);
	int (*CC_SPTR(ccmode_gcm, finalize))(ccgcm_ctx *key, size_t tag_nbytes, void *tag);
	int (*CC_SPTR(ccmode_gcm, reset))(ccgcm_ctx *ctx);
	const void *custom;
};

/* modes this interface names but does not use */
struct ccmode_cfb;
struct ccmode_cfb8;
struct ccmode_ofb;
struct ccmode_ccm;
struct ccmode_omac;
struct ccmode_siv;
struct ccmode_siv_hmac;

#endif /* _CORECRYPTO_CCMODE_IMPL_H_ */

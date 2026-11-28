// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccmode.h: storage for mode contexts, and the calls that go through a mode's table. */
#ifndef _CORECRYPTO_CCMODE_H_
#define _CORECRYPTO_CCMODE_H_

#include <corecrypto/cc.h>
#include <corecrypto/ccmode_impl.h>

/* ECB */
#define ccecb_ctx_decl(_size_, _name_)  cc_ctx_decl_vla(ccecb_ctx, _size_, _name_)
#define ccecb_ctx_clear(_size_, _name_) cc_clear(_size_, _name_)
size_t ccecb_context_size(const struct ccmode_ecb *mode);
size_t ccecb_block_size(const struct ccmode_ecb *mode);
int ccecb_init(const struct ccmode_ecb *mode, ccecb_ctx *ctx, size_t key_len, const void *key);
int ccecb_update(const struct ccmode_ecb *mode, const ccecb_ctx *ctx, size_t nblocks, const void *in, void *out);

/* CBC */
#define cccbc_ctx_decl(_size_, _name_)  cc_ctx_decl_vla(cccbc_ctx, _size_, _name_)
#define cccbc_ctx_clear(_size_, _name_) cc_clear(_size_, _name_)
#define cccbc_iv_decl(_size_, _name_)   cc_ctx_decl_vla(cccbc_iv, _size_, _name_)
#define cccbc_iv_clear(_size_, _name_)  cc_clear(_size_, _name_)
size_t cccbc_context_size(const struct ccmode_cbc *mode);
size_t cccbc_block_size(const struct ccmode_cbc *mode);
int cccbc_init(const struct ccmode_cbc *mode, cccbc_ctx *ctx, size_t key_len, const void *key);
int cccbc_copy_iv(cccbc_iv *iv_ctx, const void *iv, size_t len);
int cccbc_clear_iv(cccbc_iv *iv_ctx, size_t len);
int cccbc_set_iv(const struct ccmode_cbc *mode, cccbc_iv *iv_ctx, const void *iv);
int cccbc_update(const struct ccmode_cbc *mode, const cccbc_ctx *ctx, cccbc_iv *iv, size_t nblocks, const void *in,
    void *out);

/* CTR */
#define ccctr_ctx_decl(_size_, _name_)  cc_ctx_decl_vla(ccctr_ctx, _size_, _name_)
#define ccctr_ctx_clear(_size_, _name_) cc_clear(_size_, _name_)
size_t ccctr_context_size(const struct ccmode_ctr *mode);
size_t ccctr_block_size(const struct ccmode_ctr *mode);
int ccctr_init(const struct ccmode_ctr *mode, ccctr_ctx *ctx, size_t key_len, const void *key, const void *iv);
int ccctr_update(const struct ccmode_ctr *mode, ccctr_ctx *ctx, size_t nbytes, const void *in, void *out);

/* XTS */
#define ccxts_ctx_decl(_size_, _name_)    cc_ctx_decl_vla(ccxts_ctx, _size_, _name_)
#define ccxts_ctx_clear(_size_, _name_)   cc_clear(_size_, _name_)
#define ccxts_tweak_decl(_size_, _name_)  cc_ctx_decl_vla(ccxts_tweak, _size_, _name_)
#define ccxts_tweak_clear(_size_, _name_) cc_clear(_size_, _name_)
size_t ccxts_context_size(const struct ccmode_xts *mode);
size_t ccxts_block_size(const struct ccmode_xts *mode);
int ccxts_init(const struct ccmode_xts *mode, ccxts_ctx *ctx, size_t key_nbytes, const void *data_key,
    const void *tweak_key);
int ccxts_set_tweak(const struct ccmode_xts *mode, ccxts_ctx *ctx, ccxts_tweak *tweak, const void *iv);
void *ccxts_update(const struct ccmode_xts *mode, ccxts_ctx *ctx, ccxts_tweak *tweak, size_t nblocks, const void *in,
    void *out);

/*
 * GCM. Call order: init, set_iv (or init_with_iv), aad*, update*, finalize, then reset before the next IV. An encryptor's
 * finalize writes the tag; a decryptor's compares it with the one given and fails when they differ. A context made by
 * init_with_iv takes its next IV from inc_iv (the IV's last 8 bytes counted up, big-endian) after a reset.
 */
#define ccgcm_ctx_decl(_size_, _name_)  cc_ctx_decl_vla(ccgcm_ctx, _size_, _name_)
#define ccgcm_ctx_clear(_size_, _name_) cc_clear(_size_, _name_)
#define CCGCM_IV_NBYTES       12
#define CCGCM_BLOCK_NBYTES    16
#define CCGCM_TEXT_MAX_NBYTES ((1ULL << 36) - 32ULL)
size_t ccgcm_context_size(const struct ccmode_gcm *mode);
size_t ccgcm_block_size(const struct ccmode_gcm *mode);
int ccgcm_init(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key);
int ccgcm_init_with_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t key_nbytes, const void *key,
    const void *iv);
int ccgcm_set_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t iv_nbytes, const void *iv);
int ccgcm_inc_iv(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, void *iv);
int ccgcm_aad(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t nbytes, const void *additional_data);
int ccgcm_update(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t nbytes, const void *in, void *out);
int ccgcm_finalize(const struct ccmode_gcm *mode, ccgcm_ctx *ctx, size_t tag_nbytes, void *tag);
int ccgcm_reset(const struct ccmode_gcm *mode, ccgcm_ctx *ctx);

#endif /* _CORECRYPTO_CCMODE_H_ */

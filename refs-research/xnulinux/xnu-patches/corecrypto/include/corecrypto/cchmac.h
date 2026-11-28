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
 * cchmac.h: HMAC over a ccdigest_info. Its context is a digest context (the inner hash), padded to CC_MAX_ALIGNMENT,
 * followed by the outer hash's state.
 */
#ifndef _CORECRYPTO_CCHMAC_H_
#define _CORECRYPTO_CCHMAC_H_

#include <corecrypto/cc.h>
#include <corecrypto/ccdigest.h>

struct cchmac_ctx {
	uint8_t b[1];
} CC_ALIGNED(8);
typedef struct cchmac_ctx *cchmac_ctx_t;

#define cchmac_ctx_size(STATE_SIZE, BLOCK_SIZE) \
	(cc_pad_align(ccdigest_ctx_size(STATE_SIZE, BLOCK_SIZE)) + (STATE_SIZE))
#define cchmac_di_size(_di_) (cchmac_ctx_size((_di_)->state_size, (_di_)->block_size))
#define cchmac_ctx_n(STATE_SIZE, BLOCK_SIZE) ccn_nof_size(cchmac_ctx_size((STATE_SIZE), (BLOCK_SIZE)))

#define cchmac_ctx_decl(STATE_SIZE, BLOCK_SIZE, _name_) \
	cc_ctx_decl_vla(struct cchmac_ctx, cchmac_ctx_size(STATE_SIZE, BLOCK_SIZE), _name_)
#define cchmac_ctx_clear(STATE_SIZE, BLOCK_SIZE, _name_) cc_clear(cchmac_ctx_size(STATE_SIZE, BLOCK_SIZE), _name_)
#define cchmac_di_decl(_di_, _name_)  cchmac_ctx_decl((_di_)->state_size, (_di_)->block_size, _name_)
#define cchmac_di_clear(_di_, _name_) cchmac_ctx_clear((_di_)->state_size, (_di_)->block_size, _name_)

#define cchmac_digest_ctx(_di_, HC) ((ccdigest_ctx_t)(HC))
#define cchmac_ostate(_di_, HC)     ((ccdigest_state_t)(((cchmac_ctx_t)(HC))->b + cc_pad_align(ccdigest_di_size(_di_))))
#define cchmac_ostate8(_di_, HC)    (ccdigest_u8(cchmac_ostate(_di_, HC)))
#define cchmac_ostate32(_di_, HC)   (ccdigest_u32(cchmac_ostate(_di_, HC)))
#define cchmac_ostate64(_di_, HC)   (ccdigest_u64(cchmac_ostate(_di_, HC)))
#define cchmac_istate(_di_, HC)     ccdigest_state(_di_, ((ccdigest_ctx_t)(HC)))
#define cchmac_istate8(_di_, HC)    ccdigest_u8(cchmac_istate(_di_, HC))
#define cchmac_istate32(_di_, HC)   ccdigest_u32(cchmac_istate(_di_, HC))
#define cchmac_istate64(_di_, HC)   ccdigest_u64(cchmac_istate(_di_, HC))
#define cchmac_data(_di_, HC)       ccdigest_data(_di_, ((ccdigest_ctx_t)(HC)))
#define cchmac_num(_di_, HC)        ccdigest_num(_di_, ((ccdigest_ctx_t)(HC)))
#define cchmac_nbits(_di_, HC)      ccdigest_nbits(_di_, ((ccdigest_ctx_t)(HC)))

void cchmac_init(const struct ccdigest_info *di, cchmac_ctx_t ctx, size_t key_len, const void *key);
void cchmac_update(const struct ccdigest_info *di, cchmac_ctx_t ctx, size_t data_len, const void *data);
void cchmac_final(const struct ccdigest_info *di, cchmac_ctx_t ctx, unsigned char *mac);
void cchmac(const struct ccdigest_info *di, size_t key_len, const void *key, size_t data_len, const void *data,
    unsigned char *mac);

#endif /* _CORECRYPTO_CCHMAC_H_ */

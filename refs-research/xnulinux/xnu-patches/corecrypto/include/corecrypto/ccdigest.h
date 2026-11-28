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
 * ccdigest.h: a hash function as a table (struct ccdigest_info) of its sizes, initial state and compression function,
 * and the context every caller allocates for it:
 *   [ uint64_t bits hashed ][ state (state_size) ][ pending input (block_size) ][ unsigned int pending bytes ]
 */
#ifndef _CORECRYPTO_CCDIGEST_H_
#define _CORECRYPTO_CCDIGEST_H_

#include <corecrypto/cc.h>
#include <corecrypto/ccn.h>

struct ccdigest_ctx {
	uint8_t state[1];
} CC_ALIGNED(8);
typedef struct ccdigest_ctx *ccdigest_ctx_t;

struct ccdigest_state {
	uint8_t state[1];
} CC_ALIGNED(8);
typedef struct ccdigest_state *ccdigest_state_t;

struct ccdigest_info {
	size_t output_size;
	size_t state_size;
	size_t block_size;
	size_t oid_size;
	const unsigned char *oid;
	const void *initial_state;
	void (*CC_SPTR(ccdigest_info, compress))(ccdigest_state_t state, size_t nblocks, const void *data);
	void (*CC_SPTR(ccdigest_info, final))(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *digest);
	cc_impl_t impl;
	void (*CC_SPTR(ccdigest_info, compress_parallel))(ccdigest_state_t state1, size_t nblocks1, const void *data1,
	    ccdigest_state_t state2, size_t nblocks2, const void *data2);
};

#define ccdigest_ctx_size(_state_size_, _block_size_) \
	((_state_size_) + sizeof(uint64_t) + (_block_size_) + sizeof(unsigned int))
#define ccdigest_di_size(_di_) (ccdigest_ctx_size((_di_)->state_size, (_di_)->block_size))

#define ccdigest_ctx_decl(_state_size_, _block_size_, _name_) \
	cc_ctx_decl(struct ccdigest_ctx, ccdigest_ctx_size(_state_size_, _block_size_), _name_)
#define ccdigest_ctx_clear(_state_size_, _block_size_, _name_) \
	cc_clear(ccdigest_ctx_size(_state_size_, _block_size_), _name_)
#define ccdigest_di_decl(_di_, _name_) cc_ctx_decl_vla(struct ccdigest_ctx, ccdigest_di_size(_di_), _name_)
#define ccdigest_di_clear(_di_, _name_) cc_clear(ccdigest_di_size(_di_), _name_)

/* the fields of a context */
#define ccdigest_nbits(_di_, _ctx_)  (*((uint64_t *)((ccdigest_ctx_t)(_ctx_))->state))
#define ccdigest_state(_di_, _ctx_)  ((ccdigest_state_t)(((ccdigest_ctx_t)(_ctx_))->state + sizeof(uint64_t)))
#define ccdigest_data(_di_, _ctx_)   (((ccdigest_ctx_t)(_ctx_))->state + (_di_)->state_size + sizeof(uint64_t))
#define ccdigest_num(_di_, _ctx_) \
	(*((unsigned int *)(((ccdigest_ctx_t)(_ctx_))->state + (_di_)->state_size + sizeof(uint64_t) + (_di_)->block_size)))

#define ccdigest_u8(_state_)  ((uint8_t *)((ccdigest_state_t)(_state_)))
#define ccdigest_u32(_state_) ((uint32_t *)((ccdigest_state_t)(_state_)))
#define ccdigest_u64(_state_) ((uint64_t *)((ccdigest_state_t)(_state_)))
#define ccdigest_ccn(_state_) ((cc_unit *)((ccdigest_state_t)(_state_)))
#define ccdigest_state_u8(_di_, _ctx_)  ccdigest_u8(ccdigest_state((_di_), (_ctx_)))
#define ccdigest_state_u32(_di_, _ctx_) ccdigest_u32(ccdigest_state((_di_), (_ctx_)))
#define ccdigest_state_u64(_di_, _ctx_) ccdigest_u64(ccdigest_state((_di_), (_ctx_)))
#define ccdigest_state_ccn(_di_, _ctx_) ccdigest_ccn(ccdigest_state((_di_), (_ctx_)))

void ccdigest_init(const struct ccdigest_info *di, ccdigest_ctx_t ctx);
void ccdigest_update(const struct ccdigest_info *di, ccdigest_ctx_t ctx, size_t len, const void *data);

CC_INLINE
void
ccdigest_final(const struct ccdigest_info *di, ccdigest_ctx_t ctx, unsigned char *digest)
{
	di->final(di, ctx, digest);
}

void ccdigest(const struct ccdigest_info *di, size_t len, const void *data, void *digest);

/* DER-encoded object identifiers of the digests (tag and length included) */
#define CC_DIGEST_OID_MD5    ((const unsigned char *)"\x06\x08\x2A\x86\x48\x86\xF7\x0D\x02\x05")
#define CC_DIGEST_OID_SHA1   ((const unsigned char *)"\x06\x05\x2b\x0e\x03\x02\x1a")
#define CC_DIGEST_OID_SHA224 ((const unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x04")
#define CC_DIGEST_OID_SHA256 ((const unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x01")
#define CC_DIGEST_OID_SHA384 ((const unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x02")
#define CC_DIGEST_OID_SHA512 ((const unsigned char *)"\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x03")

#endif /* _CORECRYPTO_CCDIGEST_H_ */

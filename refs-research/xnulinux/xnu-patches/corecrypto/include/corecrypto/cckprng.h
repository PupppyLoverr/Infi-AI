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
 * cckprng.h: the kernel's random generator, as a provider registers it (register_and_init_prng()): a table of functions
 * on a context the provider owns. The kernel keeps only a pointer to the context.
 */
#ifndef _CORECRYPTO_CCKPRNG_H_
#define _CORECRYPTO_CCKPRNG_H_

#include <corecrypto/cc.h>
#if CC_KERNEL
#include <kern/locks.h>	/* the kernel's PRNG users (osfmk/prng/random.h and those who include it) take its locks from here */
#endif

/* fills entropy with up to *entropy_nbytes bytes and sets *entropy_nbytes to how many it wrote; arg is the provider's */
typedef int32_t (*cckprng_getentropy)(size_t *entropy_nbytes, void *entropy, void *arg);

#define CCKPRNG_SEED_NBYTES         32
#define CCKPRNG_GENERATE_MAX_NBYTES 256

struct cckprng_ctx;

struct cckprng_funcs {
	void (*CC_SPTR(cckprng_funcs, init))(struct cckprng_ctx *ctx, size_t seed_nbytes, const void *seed,
	    size_t nonce_nbytes, const void *nonce, cckprng_getentropy getentropy, void *getentropy_arg);
	void (*CC_SPTR(cckprng_funcs, initgen))(struct cckprng_ctx *ctx, unsigned gen_idx);
	void (*CC_SPTR(cckprng_funcs, reseed))(struct cckprng_ctx *ctx, size_t nbytes, const void *seed);
	void (*CC_SPTR(cckprng_funcs, refresh))(struct cckprng_ctx *ctx);
	void (*CC_SPTR(cckprng_funcs, generate))(struct cckprng_ctx *ctx, unsigned gen_idx, size_t nbytes, void *out);
	void (*CC_SPTR(cckprng_funcs, init_with_getentropy))(struct cckprng_ctx *ctx, unsigned max_ngens,
	    size_t seed_nbytes, const void *seed, size_t nonce_nbytes, const void *nonce, cckprng_getentropy getentropy,
	    void *getentropy_arg);
};

#endif /* _CORECRYPTO_CCKPRNG_H_ */

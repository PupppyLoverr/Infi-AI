// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* ccrng.h: a random generator is a struct whose first member is its generate function. */
#ifndef _CORECRYPTO_CCRNG_H_
#define _CORECRYPTO_CCRNG_H_

#include <corecrypto/cc.h>

#define CCRNG_STATE_COMMON \
	int (*CC_SPTR(ccrng_state, generate))(struct ccrng_state *rng, size_t outlen, void *out);

struct ccrng_state {
	CCRNG_STATE_COMMON
};

/* the provider's generator (NULL with *error set on failure) */
struct ccrng_state *ccrng(int *error);

#define ccrng_generate(rng, outlen, out) ((rng)->generate((struct ccrng_state *)(rng), (outlen), (out)))

/* a uniform value in [0, bound) */
int ccrng_uniform(struct ccrng_state *rng, uint64_t bound, uint64_t *rand);

#endif /* _CORECRYPTO_CCRNG_H_ */

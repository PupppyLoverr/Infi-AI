// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* Memory for Mbed TLS (RSA's big numbers), and the random numbers: the host's. */
#include "cc_internal.h"
#include <mbedtls/chacha20.h>
#include <mbedtls/platform_util.h>
#if !CC_TEST_USERSPACE
#include <IOKit/IOLib.h>
#endif

/* each block carries its size, which IOFreeData needs */
struct block {
	size_t size;
	uint64_t data[];
};

void *
xnulinux_cc_calloc(size_t n, size_t size)
{
	struct block *b;
	size_t bytes;

	if (n != 0 && size > (SIZE_MAX - sizeof(struct block)) / n) {
		return NULL;
	}
	bytes = sizeof(struct block) + n * size;
#if CC_TEST_USERSPACE
	b = calloc(1, bytes);
#else
	b = IOMallocZeroData(bytes);
#endif
	if (b == NULL) {
		return NULL;
	}
#if CC_TEST_USERSPACE
	xc_test_alloc(b, bytes);
#endif
	b->size = bytes;
	return b->data;
}

void
xnulinux_cc_free(void *p)
{
	struct block *b;
	size_t bytes;

	if (p == NULL) {
		return;
	}
	b = (struct block *)((uint8_t *)p - offsetof(struct block, data));
	bytes = b->size;	/* cc_clear zeroes the size too */
	cc_clear(bytes, b);
#if CC_TEST_USERSPACE
	xc_test_free(b, bytes);
#else
	IOFreeData(b, bytes);
#endif
}

/* Mbed TLS formats OIDs as text only for display, which nothing here asks for */
int
xnulinux_cc_snprintf(char *s, size_t n, const char *format, ...)
{
	(void)format;
	if (n > 0) {
		s[0] = 0;
	}
	return -1;
}

/* the generator every ccrng caller gets: the host's */
static int
host_generate(struct ccrng_state *rng, size_t outlen, void *out)
{
	(void)rng;
	xc_random(out, outlen);
	return 0;
}

static struct ccrng_state host_rng = { .generate = host_generate };

struct ccrng_state *
xc_rng(int *error)
{
	if (error != NULL) {
		*error = 0;
	}
	return &host_rng;
}

/* uniform in [0, bound), by rejection (no modulo bias) */
int
xc_rng_uniform(struct ccrng_state *rng, uint64_t bound, uint64_t *rand)
{
	uint64_t v, limit;

	if (bound == 0) {
		return CCERR_PARAMETER;
	}
	limit = -bound % bound;
	do {
		int rc = ccrng_generate(rng, sizeof(v), &v);
		if (rc != 0) {
			return rc;
		}
	} while (v < limit);
	*rand = v % bound;
	return 0;
}

/*
 * kmem's generator (crypto_random_kmem_*): its contract is that generating takes no lock, which a call to the host
 * cannot promise. Keyed from the host at init (which may lock); then ChaCha20 with fast key erasure: each block's first
 * half is the next key, its second half the output. One per CPU, used with preemption off (osfmk/vm/vm_map_store.c).
 */
struct kmem_rng {
	struct ccrng_state rng;	/* first: the context is the ccrng */
	uint8_t key[32];
	uint8_t out[32];
	uint32_t left;		/* unused bytes at the end of out */
};

static int
kmem_generate(struct ccrng_state *rng, size_t outlen, void *out)
{
	static const uint8_t nonce[12], zero[64];
	struct kmem_rng *k = (struct kmem_rng *)rng;
	uint8_t *p = out, block[64];

	while (outlen > 0) {
		size_t n;
		if (k->left == 0) {
			(void)mbedtls_chacha20_crypt(k->key, nonce, 0, sizeof(block), zero, block);
			memcpy(k->key, block, sizeof(k->key));
			memcpy(k->out, block + sizeof(k->key), sizeof(k->out));
			mbedtls_platform_zeroize(block, sizeof(block));
			k->left = sizeof(k->out);
		}
		n = outlen < k->left ? outlen : k->left;
		memcpy(p, k->out + sizeof(k->out) - k->left, n);
		mbedtls_platform_zeroize(k->out + sizeof(k->out) - k->left, n);
		k->left -= (uint32_t)n;
		p += n;
		outlen -= n;
	}
	return 0;
}

_Static_assert(sizeof(struct kmem_rng) <= 256, "kmem's generator must fit CRYPTO_RANDOM_MAX_CTX_SIZE");

size_t
xc_kmem_rng_size(void)
{
	return sizeof(struct kmem_rng);
}

void
xc_kmem_rng_init(void *ctx)
{
	struct kmem_rng *k = ctx;

	k->rng.generate = kmem_generate;
	xc_random(k->key, sizeof(k->key));
	k->left = 0;
}

// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* cc.h: alignment, context storage, pointer signing, and the two helpers every user needs. */
#ifndef _CORECRYPTO_CC_H_
#define _CORECRYPTO_CC_H_

#include <corecrypto/cc_config.h>
#include <corecrypto/cc_error.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdarg.h>
#if CC_KERNEL
#include <kern/assert.h>
#else
#include <assert.h>
#endif

#define cc_concat_(a, b) a##b
#define cc_concat(a, b)  cc_concat_(a, b)
#define cc_static_assert(e, m) _Static_assert(e, m)
#define cc_assert(x)

/* which implementation a mode or digest is (its value is the provider's business) */
typedef enum cc_impl {
	CC_IMPL_UNKNOWN = 0,
} cc_impl_t;

/*
 * Function pointers in the interface's tables are signed, with the process-independent code key and a discriminator
 * of "cc_" followed by the struct's and the field's names, when the kernel uses pointer authentication.
 */
#if __has_feature(ptrauth_calls) && CC_KERNEL
#include <ptrauth.h>
#define CC_SPTR(_sn_, _n_) \
	__ptrauth(ptrauth_key_process_independent_code, 1, ptrauth_string_discriminator("cc_" #_sn_ #_n_)) _n_
#else
#define CC_SPTR(_sn_, _n_) _n_
#endif

/* an opaque type of _alignment_ bytes, aligned to them: contexts are arrays of it */
#define cc_aligned_struct(_alignment_) \
	typedef struct { uint8_t b[_alignment_]; } CC_ALIGNED(_alignment_)

#if defined(__BIGGEST_ALIGNMENT__)
#define CC_MAX_ALIGNMENT ((size_t)__BIGGEST_ALIGNMENT__)
#else
#define CC_MAX_ALIGNMENT ((size_t)16)
#endif
#define cc_pad_align(_size_) (((_size_) + CC_MAX_ALIGNMENT - 1) & (~(CC_MAX_ALIGNMENT - 1)))

/* how many _type_ hold _size_ bytes, and their size */
#define cc_ctx_n(_type_, _size_) (((_size_) + sizeof(_type_) - 1) / sizeof(_type_))

#define CC_IGNORE_VLA_WARNINGS     _Pragma("GCC diagnostic push") _Pragma("GCC diagnostic ignored \"-Wvla\"")
#define CC_RESTORE_VLA_WARNINGS    _Pragma("GCC diagnostic pop")

#define cc_ctx_sizeof(_type_, _size_) \
	CC_IGNORE_VLA_WARNINGS sizeof(_type_[cc_ctx_n(_type_, _size_)]) CC_RESTORE_VLA_WARNINGS

/* a context of _size_ bytes on the stack, or as a struct field */
#define cc_ctx_decl(_type_, _size_, _name_) \
	CC_IGNORE_VLA_WARNINGS _type_ _name_[cc_ctx_n(_type_, _size_)]; CC_RESTORE_VLA_WARNINGS
#define cc_ctx_decl_field(_type_, _size_, _name_) _type_ _name_[cc_ctx_n(_type_, _size_)]
#define cc_ctx_decl_vla(_type_, _size_, _name_) \
	CC_IGNORE_VLA_WARNINGS cc_ctx_decl(_type_, _size_, _name_); CC_RESTORE_VLA_WARNINGS

/* zero len bytes at dst, in a way the compiler keeps */
CC_NONNULL((2))
void cc_clear(size_t len, void *dst);

#define cc_copy(_size_, _dst_, _src_) memcpy(_dst_, _src_, _size_)

/* 0 when the num bytes at ptr1 and ptr2 are equal (num > 0), otherwise 1; the time taken depends on num only */
CC_NONNULL((2, 3))
int cc_cmp_safe(size_t num, const void *ptr1, const void *ptr2);

#endif /* _CORECRYPTO_CC_H_ */

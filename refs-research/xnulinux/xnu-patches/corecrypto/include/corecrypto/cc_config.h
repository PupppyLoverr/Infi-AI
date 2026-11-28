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
 * xnulinux's corecrypto interface: the types, macros and declarations through which XNU, its kexts and a crypto kext
 * (com.apple.kec.corecrypto, which registers with register_crypto_functions()) talk to each other. Written for xnulinux;
 * the layouts are those that interface has (a crypto kext built for macOS and this kernel must agree on them).
 *
 * cc_config.h: the build environment.
 */
#ifndef _CORECRYPTO_CC_CONFIG_H_
#define _CORECRYPTO_CC_CONFIG_H_

#if defined(KERNEL) && (KERNEL)
#define CC_KERNEL 1
#else
#define CC_KERNEL 0
#endif

#if defined(__LP64__) && __LP64__
#define CCN_UNIT_SIZE 8
#else
#define CCN_UNIT_SIZE 4
#endif

#define CC_ALIGNED(x)       __attribute__((aligned(x)))
#define CC_INLINE           static inline
#define CC_NONNULL(N)       __attribute__((__nonnull__ N))
#define CC_NONNULL_ALL      __attribute__((__nonnull__))
#define CC_UNUSED           __attribute__((unused))
#define CC_PURE             __attribute__((__pure__))
#define CC_WARN_RESULT      __attribute__((__warn_unused_result__))

/* bounds annotations: not used by this kernel's build */
#define CC_PTRCHECK 0
#define CC_PTRCHECK_CAPABLE_HEADER()
#define cc_counted_by(x)
#define cc_sized_by(x)
#define cc_ended_by(x)
#define cc_indexable
#define cc_bidi_indexable
#define cc_single
#define cc_unsafe_indexable
#define cc_cstring
#define cc_unsafe_forge_bidi_indexable(p, n) (p)
#define cc_unsafe_forge_single(p)            (p)

#endif /* _CORECRYPTO_CC_CONFIG_H_ */

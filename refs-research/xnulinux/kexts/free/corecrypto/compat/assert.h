// SPDX-License-Identifier: APSL-2.0
/* For Mbed TLS in the kernel (-idirafter): its asserts are debugging aids, off as in a release build. */
#ifndef XNULINUX_CC_COMPAT_ASSERT_H
#define XNULINUX_CC_COMPAT_ASSERT_H
#ifndef assert
#define assert(x) ((void)0)
#endif
#ifndef static_assert
#define static_assert _Static_assert
#endif
#endif

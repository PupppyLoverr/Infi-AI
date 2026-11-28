/* SPDX-License-Identifier: APSL-2.0 */
/* User space stand-in for <linux/module.h> (see ../test.c): what xnu_main.c uses of the module ABI. */
#pragma once
#define module_param(name, type, perm)
#define module_init(fn)
#define module_exit(fn)
#define MODULE_LICENSE(x)
#define MODULE_DESCRIPTION(x)
#define __init
#define __exit
#define READ_ONCE(x) (*(volatile __typeof__(x) *)&(x))
#define WRITE_ONCE(x, v) (*(volatile __typeof__(x) *)&(x) = (v))
#define likely(x) __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define __always_inline inline __attribute__((always_inline))
void test_bug(void) __attribute__((noreturn));
#define BUG() test_bug()

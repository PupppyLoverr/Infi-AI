/* SPDX-License-Identifier: GPL-2.0-only */
/* Shared between fki's own files (not part of the interface in fki.h). */
#ifndef FKI_INTERNAL_H
#define FKI_INTERNAL_H

/* Run fn(arg) where Linux may run it (a Linux-compliant stack), keeping the caller's exception mask. */
void fki_linux_runs_ext(void (*fn)(void *), void *arg);

/* Architecture backend hooks (fki_arm64.c / fki_l4h.c). */
int fki_arch_init(void);
void fki_arch_exit(void);
void fki_arch_linux_runs(void (*fn)(void *), void *arg);
void fki_fpsimd_forget(void);

int fki_cpu_map(int cpu);
int fki_hostcall_init(void);
void fki_hostcall_exit(void);
#endif

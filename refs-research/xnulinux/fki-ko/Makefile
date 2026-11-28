# SPDX-License-Identifier: GPL-2.0-only
obj-m += fki.o
fki-y := fki_common.o fki_hostcall.o
ifeq ($(CONFIG_L4H),y)
fki-y += fki_l4h.o
else
fki-$(CONFIG_ARM64) += fki_arm64.o
endif

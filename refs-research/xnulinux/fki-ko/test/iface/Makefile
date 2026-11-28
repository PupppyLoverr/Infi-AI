# SPDX-License-Identifier: GPL-2.0-only
# Built by run.sh (the module against the running kernel and ../../fki.h; the helpers with the host's compiler).
obj-m += ifacetest.o
ifacetest-y := main.o asm.o
ccflags-y += -I$(src)/fki-ko

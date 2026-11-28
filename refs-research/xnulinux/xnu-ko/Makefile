# SPDX-License-Identifier: APSL-2.0
# xnu.ko: the APSL side. Out-of-tree, non-GPL module that only uses fki.h.
obj-m += xnu.o
xnu-y := xnu_main.o macho.o adt.o world.o
ccflags-y += -Wno-declaration-after-statement

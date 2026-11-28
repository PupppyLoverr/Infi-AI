#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM, after scripts/vm-xnu-rebuild.sh: a kernel collection with none of Apple's kexts. The hosted kernel and xnulinux's
# own kexts (XnuCPU, XnuDisk, XnuDrm), plus the kexts a Darwin userland needs, built from PureDarwin's sources as arm64e with
# this kernel's own compile flags and headers (scripts/free-kc/kcc.py): a permissive AMFI, pthread, IOStorageFamily,
# HFSEncodings and hfs; and xnulinux's own corecrypto kext (kexts/free/corecrypto: Mbed TLS behind the kernel's corecrypto
# interface). It boots PureDarwin's userland from an HFS+ disk (scripts/mk-puredarwin-disk.sh, which prints its pd.conf):
#   MAC_CONF=pd.conf DISK=pd.img IMAGE=xnu/kc-free.bin TRUSTCACHE= GPU=off scripts/xnulinux-boot.sh
# Sources are fetched at pinned revisions (Mbed TLS: kexts/free/corecrypto/mbedtls.rev).
#
# Nothing of Apple's is read: the collection is linked into an empty one that mk-template.py makes from layout numbers
# (layout.json) and XNU's own pseudo-extensions.
set -euo pipefail
POC=$(cd "$(dirname "$0")/../.." && pwd)
DVM=${DVM:-$HOME/w/dvm}; W=${FREE_KC_WORK:-$HOME/w/free-kc}; OUT=${OUT:-/lib/firmware/xnu/kc-free.bin}
mkdir -p $W; W=$(cd $W && pwd); DVM=$(cd $DVM && pwd)	# every step below runs in another directory: no relative names
PD_URL=https://github.com/AFK-surf/PureDarwin PD_REV=b3b4b2812fdd1c5c251dc3089b5a1a9e00d29970
MBEDTLS_URL=https://github.com/Mbed-TLS/mbedtls MBEDTLS_REV=$(sed -n 's/^MBEDTLS_REV=//p' $POC/kexts/free/corecrypto/mbedtls.rev)
export DVM NETINC=${NETINC:-$POC/build/netinc}
K="python3 $POC/scripts/free-kc/kcc.py --macos"; O=$W/obj; S=$W/src; F=$POC/kexts/free
mkdir -p $S $O

fetch() {	# repo rev dir [paths...]: a shallow checkout of one revision, only the paths given
	local url=$1 rev=$2 dir=$3; shift 3
	[ "$(git -C $dir rev-parse HEAD 2>/dev/null)" = "$rev" ] && return
	rm -rf $dir; git init -q $dir
	[ $# -gt 0 ] && { git -C $dir sparse-checkout init --no-cone; printf '%s\n' "$@" | git -C $dir sparse-checkout set --stdin; }
	git -C $dir fetch -q --depth 1 $url $rev && git -C $dir checkout -q FETCH_HEAD
}
E=src/Kernel/Extensions
fetch $PD_URL $PD_REV $S/pd /$E/IOStorageFamily/ /$E/HFSEncodings/ /$E/hfs/ /$E/pthread/ /$E/amfi/ /src/Kernel/xnu/EXTERNAL_HEADERS/ \
	/src/Libraries/libSystem/pthread/include/ /src/Libraries/libSystem/pthread/private/
fetch $MBEDTLS_URL $MBEDTLS_REV $S/mbedtls /include/ /library/
E=$S/pd/src/Kernel/Extensions

# this kernel's Kernel.framework (xnu installhdrs into objects of its own, k27h-*), what --kext-view builds against
KF=$DVM/out/x12377/build/xnu/k27h-dst/System/Library/Frameworks/Kernel.framework/Versions/A
if [ ! -d $KF/PrivateHeaders ]; then
	HO=$DVM/out/x12377/build/xnu/k27h-obj; mkdir -p $HO
	( cd $DVM/k27/iig && find DEVELOPMENT_ARM64_BCM2837 -type f ) | while read -r f; do	# DriverKit headers come from iig's output
		mkdir -p $HO/$(dirname $f); [ -e $HO/$f ] || cp $DVM/k27/iig/$f $HO/$f; done
	( cd $DVM/out/x12377 && . $DVM/k27/x12377/env.sh && XNU_ARCH=arm64e XNU_ARM_DT_PLATFORM=1 XNU_ARMCPU=1 XNU_TREE=../k27/xnu KDK=kdk \
		XNU_OBJ=k27h XNU_BUILD_TARGET=installhdrs XNU_DEPLOYMENT_TARGET_FLAGS="-miphoneos-version-min=26.0 -DXNU_TARGET_OS_OSX" \
		bash $DVM/k27/x12377/build-kernel.sh > $W/installhdrs.log 2>&1 ) || { tail -20 $W/installhdrs.log; exit 1; }
fi

# the kexts (each step stops the build when a source fails)
# corecrypto: the kernel's own view of its headers (it registers with libkern's and osfmk's interfaces), not the macOS target
C=$F/corecrypto
( cd $C && python3 $POC/scripts/free-kc/kcc.py $O/corecrypto -I $C -I $S/mbedtls/include -I $S/mbedtls/library --after $C/compat \
	-D 'MBEDTLS_CONFIG_FILE="mbedtls_config_xnulinux.h"' cc_kext.c cc_digest.c cc_aes.c cc_des.c cc_chachapoly.c cc_rsa.c \
	cc_platform.c cc_selftest.c $(for f in $(sed -n 's/^MBEDTLS_SOURCES=//p' $C/mbedtls.rev); do echo $S/mbedtls/library/$f; done) )
# PureDarwin's AMFI stub, with PureDarwin's CoreEntitlements headers (it declares their coreentitlements_t itself) and
# TrustCache headers (after this kernel's own)
mkdir -p $W/ce-inc; ln -sfn $S/pd/src/Kernel/xnu/EXTERNAL_HEADERS/CoreEntitlements $W/ce-inc/CoreEntitlements
( cd $E/amfi && $K $O/amfi --kext-view -I $F/compat/include -I $W/ce-inc \
	--after $S/pd/src/Kernel/xnu/EXTERNAL_HEADERS -D XNU_KERNEL_PRIVATE -D MACH_KERNEL_PRIVATE -D PRIVATE=1 \
	--cflag=-Wno-int-conversion --cflag=-Wno-implicit-function-declaration amfi_kext_main.c amfi_entitlements.cpp )
( cd $E/pthread && $K $O/pthread --kext-view -I $E/pthread/compat -I $E/pthread -I $S/pd/src/Libraries/libSystem/pthread/include \
	-I $S/pd/src/Libraries/libSystem/pthread/private -D __PTHREAD_EXPOSE_INTERNALS__ -D PRIVATE=1 -D XNU_KERNEL_PRIVATE \
	-D MACH_KERNEL_PRIVATE -D PD_FORCE_LOCAL_STDATOMIC --cflag=-Wno-int-conversion --cflag=-Wno-implicit-function-declaration \
	--cflag=-Wno-incompatible-library-redeclaration --cflag=-include$S/pd/src/Kernel/xnu/EXTERNAL_HEADERS/stdatomic.h \
	--cflag=-include$S/pd/src/Kernel/xnu/EXTERNAL_HEADERS/pd_pthread_sched.h kern_init.c kern_support.c kern_synch.c )
( cd $E/IOStorageFamily && $K $O/IOStorageFamily -I $E/IOStorageFamily/include --after $NETINC \
	--cflag=-include$DVM/out/k27/xnu/libkern/libkern/OSMalloc.h --cflag=-include$F/compat/devfs-ready.h *.cpp )
( cd $E/HFSEncodings && $K $O/HFSEncodings -I $E/HFSEncodings -I $E/hfs/core --after $NETINC *.c *.cpp )
# hfs's kext-config.h: its two compatibility declarations clash with this kernel's own headers, and the iOS target the
# kernel builds for would select HFS's iOS configuration (content protection) where the kexts are macOS's
sed -e '/^extern int tsleep(void \*chan/d; /^extern int proc_is_forcing_hfs_case_sensitivity/d' \
	-e 's|^#if (TARGET_OS_IPHONE && !TARGET_OS_SIMULATOR) // iOS (real hardware)|#if 0 // xnulinux: macOS settings|' \
	$E/hfs/core/kext-config.h > $W/hfs-kext-config.h
( cd $E/hfs && $K $O/hfs --kext-view --drop-def CONFIG_PROTECT -I $E/hfs/shim -I $E/hfs/core -I $E/hfs/hfs_encodings \
	-I $E/IOStorageFamily/include -I $NETINC --cflag=-include$W/hfs-kext-config.h core/*.c core/*.cpp )
for c in kmod-start osmalloc loadtag kalloc-type; do ( cd $F/compat && $K $O/compat/$c --kext-view $c.c ); done
python3 $POC/scripts/free-kc/genspec.py $S $O $O/specs corecrypto amfi pthread IOStorageFamily HFSEncodings hfs

N="$O/specs/corecrypto.json $O/specs/amfi.json $O/specs/pthread.json $O/specs/IOStorageFamily.json $O/specs/HFSEncodings.json $O/specs/hfs.json"
for k in XnuCPU XnuDisk XnuDrm; do N="$N $POC/kexts/$k/$k.json"; done
# the collection is linked into an empty one made from layout numbers and XNU's own pseudo-extensions (mk-template.py):
# nothing of Apple's kernelcache is read
python3 $POC/scripts/free-kc/mk-template.py $POC/scripts/free-kc/layout.json $DVM/out/k27/xnu \
	$(strings $DVM/out/k27/kernel-k27.macho | sed -n 's/^Darwin Kernel Version \([0-9.]*\):.*/\1/p' | head -1) $W/kc-template.bin
# the link (scripts/kclink, as vm-xnu-rebuild.sh links; the specs' object lists of xnulinux's kexts are relative to $DVM)
( cd $DVM && python3 $POC/scripts/kclink/kclink.py $W/kc-template.bin out/k27/kernel-k27.macho $W/kc-free.bin --native $N \
	> $W/kclink.log 2>&1 ) || { tail -5 $W/kclink.log; exit 1; }
grep -E "^wrote" $W/kclink.log
sudo cp $W/kc-free.bin $OUT && echo "kernel collection without Apple's kexts: $OUT"

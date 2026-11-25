#!/bin/bash
# x86_64-native build of the xnulinux XNU kernel, in an ubuntu:24.04 container.
# Mirrors scripts/vm-xnu-build.sh minus its arm64-host-only patches: upstream
# macOS-DroidVM targets x86_64 Linux build hosts natively (production.md:
# "XNU + collection build (x86_64 Linux works with the DroidVM toolchain)").
set -ex
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
curl -s4 https://apt.llvm.org/llvm-snapshot.gpg.key -o /etc/apt/trusted.gpg.d/apt.llvm.org.asc
echo "deb http://apt.llvm.org/noble/ llvm-toolchain-noble-21 main" | tee /etc/apt/sources.list.d/llvm21.list
apt-get update -qq
apt-get install -y -qq build-essential autoconf automake libtool pkg-config git python3 python3-yaml python3-packaging \
  cmake ninja-build meson bison flex m4 unifdef tcsh pax libbsd-dev uuid-dev libssl-dev zlib1g-dev cpio xz-utils zstd llvm-21 clang-21 lld-21 \
  libmd-dev 7zip p7zip-full wget curl >/dev/null
mkdir -p ~/w && cd ~/w
[ -d dvm ] || git clone -q https://github.com/Droid-VM/macOS-DroidVM dvm
cd dvm
python3 - <<'PY'
p='scripts/setup-env.py'; s=open(p).read()
a=s.find("    apple = linux / 'appleclang'"); b=s.find("    # bin26: the kernel's adapters")
if a>0 and b>a and 'xnupoc' not in s:
    s=s[:a]+"    # (xnupoc) no Darling/AppleClang: native_compile builds the kernel with host LLVM\n"+s[b:]
    open(p,'w').write(s)
PY
python3 scripts/setup-env.py toolchain --jobs 6
A=toolchains/linux/appleclang; mkdir -p $A
python3 compat/xnu-linux/scripts/setup-appleclang.py fetch --directory $A
[ -d $A/apple-tools ] || python3 compat/xnu-linux/scripts/setup-appleclang.py extract --directory $A
ls $A/apple-tools/Library/Developer/CommandLineTools/usr/lib/clang/*/lib/darwin/ | grep cc_kext
python3 scripts/setup-env.py x12377 --jobs 6
JOBS=6 bash k27/build-kernel27.sh 2>&1 | tail -30
ls -la out/k27/kernel-k27.macho

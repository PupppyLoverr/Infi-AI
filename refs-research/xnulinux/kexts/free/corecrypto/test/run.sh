#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Build the corecrypto kext's sources for user space with Mbed TLS (MBEDTLS: a checkout of the pinned release; fetched
# when not given) and run test.c against OpenSSL, with AddressSanitizer and UndefinedBehaviorSanitizer. On Linux, or on a
# Mac (OPENSSL: OpenSSL's prefix when the compiler does not find it by itself; Homebrew's on a Mac). On arm64 this is the
# code the hosted kernel runs (AES and SHA-256 with the CPU's instructions only: mbedtls_config_xnulinux.h).
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd); K=$D/..; I=$(cd "$K/../../../xnu-patches/corecrypto/include" && pwd)
MBEDTLS_REV=$(sed -n 's/^MBEDTLS_REV=//p' "$K/mbedtls.rev")
W=${W:-$D/build}; mkdir -p "$W"
M=${MBEDTLS:-$W/mbedtls-$MBEDTLS_REV}	# the cache is per pinned commit
if [ ! -d "$M/library" ] && [ -z "${MBEDTLS:-}" ]; then
	rm -rf "$M"; git init -q "$M" && git -C "$M" fetch -q --depth 1 https://github.com/Mbed-TLS/mbedtls "$MBEDTLS_REV" &&
		git -C "$M" checkout -q FETCH_HEAD
fi
HEAD=$(git -C "$M" rev-parse HEAD 2>/dev/null || echo "(not a git checkout)")
[ "$HEAD" = "$MBEDTLS_REV" ] || { echo "Mbed TLS at $M is $HEAD, mbedtls.rev pins $MBEDTLS_REV"; exit 1; }
LIB=$(sed -n 's/^MBEDTLS_SOURCES=//p' "$K/mbedtls.rev")
CC=${CC:-clang}
OPENSSL=${OPENSSL:-$([ "$(uname)" != Darwin ] || brew --prefix openssl@3 2>/dev/null || true)}
$CC -std=c11 -O1 -g -fsanitize=address,undefined -fno-sanitize-recover=undefined -Wall -Wextra -Wno-unused-parameter \
	-DCC_TEST_USERSPACE=1 -D_GNU_SOURCE -DMBEDTLS_CONFIG_FILE='"mbedtls_config_xnulinux.h"' -I"$I" -I"$K" -I"$M/include" -I"$M/library" \
	${OPENSSL:+-I"$OPENSSL/include" -L"$OPENSSL/lib"} \
	"$D/test.c" "$K"/cc_digest.c "$K"/cc_aes.c "$K"/cc_des.c "$K"/cc_chachapoly.c "$K"/cc_rsa.c "$K"/cc_platform.c \
	"$K"/cc_selftest.c \
	$(for f in $LIB; do echo "$M/library/$f"; done) -lcrypto -o "$W/cctest" > "$W/build.log" 2>&1 ||
	{ head -60 "$W/build.log"; exit 1; }
"$W/cctest"

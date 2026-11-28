<!-- SPDX-License-Identifier: GPL-2.0-only -->
# corecrypto: xnulinux's own interface and provider

XNU ships part of Apple's corecrypto: the interface's headers (`EXTERNAL_HEADERS/corecrypto`), a built-in subset compiled
into the kernel (`osfmk/corecrypto`, `osfmk/arm64/corecrypto`), all under Apple's corecrypto internal-use license. None of
it is used here:

| | XNU's | xnulinux's |
|---|---|---|
| The interface (types, layouts, declarations) | `EXTERNAL_HEADERS/corecrypto` (42 headers) | `xnu-patches/corecrypto/include/corecrypto` (written for xnulinux, APSL-2.0) |
| The kernel's side | `osfmk/corecrypto`: SHA-2, HMAC, HKDF, HMAC-DRBG, GCM and CBC code | `xnu-patches/corecrypto/kernel` (`osfmk/xnulinux_crypto`): no algorithm; digests and HMAC go to the registered provider, modes through their tables |
| Early random numbers (`early_random`, `read_erandom`) | an HMAC-DRBG seeded with HMAC-SHA-512 of the booter's seed | the host's generator (Linux's `get_random_bytes`, `LINUX64_CALL_RANDOM`); the boot seeds are the booter's 256 bytes as they are (`xnu-patches/0008`). `xnu.ko` waits for Linux's generator to be seeded before it takes any (`fki_random_wait`) |
| The provider (`com.apple.kec.corecrypto`) in `kc-free.bin` | (Apple's kext, in `kc-apple.bin` only) | `kexts/free/corecrypto`: Mbed TLS behind the interface |

`scripts/vm-xnu-rebuild.sh` removes XNU's three directories and copies xnulinux's in; `0008` takes the built-in files out of
the build and changes `osfmk/prng/prng_random.c`. Without the host the kernel has no early random numbers, so it boots
hosted only.

## The interface's ABI

`kc-apple.bin` still carries Apple's corecrypto kext, which registers with this kernel (`register_crypto_functions`,
`register_and_init_prng`): the structures and context layouts in the new headers are the ones that kext and the kernel's
callers agree on. `scripts/corecrypto-abi-check.sh` checks it: every kernel source that includes `<corecrypto/...>` is
compiled twice with its own recipe, once with XNU's headers (read from the k27 tree's history into a temporary directory,
never kept) and once with xnulinux's, and the objects' code, data, relocations and section sizes must be the same. All 22
such sources this configuration builds are, C and C++, xnulinux's kernel side and the PRNG's registration among them (the
one other, NFS's Kerberos, is not built). The check fails when a source other than that one has no recipe, or nothing was compared.

## The provider

`kexts/free/corecrypto` registers every function `struct crypto_functions` has:

| | |
|---|---|
| Digests | MD5, SHA-1, SHA-224/256/384/512: Mbed TLS's block functions and padding on the interface's digest contexts |
| HMAC | RFC 2104 over those digests, in the interface's HMAC context |
| AES | ECB, CBC, CTR, XTS (IEEE 1619 tweak over Mbed TLS's AES), GCM (Mbed TLS's; tags of 4, 8 and 12 to 16 bytes, SP 800-38D's); arm64 with the CPU's AES and SHA-256 instructions |
| DES, triple DES | ECB, CBC; weak keys and parity; CBC-CS3 (RFC 3962) over any CBC mode |
| ChaCha20-Poly1305 | Mbed TLS's (RFC 8439); not `chacha20poly1305_incnonce`, which nothing in the kernel calls (`CCMODE_NOT_SUPPORTED`) |
| RSA | PKCS #1 v1.5 signature verification (Mbed TLS's) |
| Random numbers | the kernel PRNG and `ccrng`: the host's generator. kmem's (`crypto_random_kmem_*`, which must take no lock once set up): ChaCha20 with fast key erasure, keyed from the host when kmem sets it up |

Mbed TLS is fetched at build time at the release `kexts/free/corecrypto/mbedtls.rev` pins (Apache-2.0); its configuration is
`mbedtls_config_xnulinux.h`. The kext runs known-answer tests when it starts and panics when one fails.

`kexts/free/corecrypto/test/run.sh` builds the same sources for Linux user space with AddressSanitizer and
UndefinedBehaviorSanitizer and tests them through the interface's calls against OpenSSL on random keys, lengths and call
splits: every digest and HMAC, AES in every mode and key size (GCM with random IV lengths, pieces of AAD and text, forged
tags; IPsec's `init_with_iv`, `reset`, `inc_iv` sequence), DES and triple DES, RFC 3962's CTS vectors, ChaCha20-Poly1305,
RSA signatures of 1024 to 4096 bits, and the uniformity of the random numbers; the edge cases (XTS key sizes, unknown digest OIDs, DES weak keys of either parity, ChaCha20-Poly1305 dropped halfway, a fresh digest context's block), and that each free names its allocation's size.

Tested in the krunkit VM: PureDarwin on `kc-free.bin` (the kext's self tests pass, the root mounts, its shell runs), and
macOS on the kernel with Apple's corecrypto kext (`kc-apple.bin`): the desktop in 36 s, `codesign -v` of an app.

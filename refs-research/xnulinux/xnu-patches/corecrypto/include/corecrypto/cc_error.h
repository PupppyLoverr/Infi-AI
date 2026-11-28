// SPDX-License-Identifier: APSL-2.0
/*
 * Copyright (c) 2026 xnulinux contributors. All rights reserved.
 *
 * @APPLE_OSREFERENCE_LICENSE_HEADER_START@
 * This file contains Original Code and/or Modifications of Original Code as defined in and that are subject to the Apple
 * Public Source License Version 2.0 (the 'License'). You may not use this file except in compliance with the License.
 * @APPLE_OSREFERENCE_LICENSE_HEADER_END@
 */
/* cc_error.h: the status codes the interface's functions return (0 is success). */
#ifndef _CORECRYPTO_CC_ERROR_H_
#define _CORECRYPTO_CC_ERROR_H_

enum {
	CCERR_OK = 0,
	CCERR_INTERNAL = -1,
	CCERR_INTEGRITY = -2,
	CCERR_PARAMETER = -7,
	CCERR_MEMORY = -8,
	CCERR_OUT_OF_ENTROPY = -10,
	CCERR_OVERFLOW = -12,
	CCRSA_INVALID_INPUT = -23,
	CCRSA_INVALID_CONFIG = -24,
	CCRSA_ENCODING_ERROR = -25,
	CCRSA_DECODING_ERROR = -26,
	CCRSA_KEY_ERROR = -28,
	CCDRBG_STATUS_ERROR = -61,
	CCDRBG_STATUS_NEED_RESEED = -62,
	CCDRBG_STATUS_PARAM_ERROR = -63,
	CCDRBG_STATUS_ABORT = -64,
	CCKPRNG_NEED_ENTROPY = -65,
	CCKPRNG_ABORT = -66,
	CCMODE_INVALID_INPUT = -67,
	CCMODE_INVALID_CALL_SEQUENCE = -68,
	CCMODE_INTEGRITY_FAILURE = -69,
	CCMODE_NOT_SUPPORTED = -70,
	CCMODE_INTERNAL_ERROR = -71,
	CCERR_CALL_SEQUENCE = -86,
	CCERR_INVALID_SIGNATURE = -146,
	CCERR_BUFFER_TOO_SMALL = -163,
	CCERR_XTS_KEYS_EQUAL = -164,
	CCERR_NOT_SUPPORTED = -173,
};

#define CCERR_VALID_SIGNATURE CCERR_OK
#define CCDRBG_STATUS_OK      CCERR_OK
#define CCKPRNG_OK            CCERR_OK

#endif /* _CORECRYPTO_CC_ERROR_H_ */

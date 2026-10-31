/* Encoder-only integer pixel kernels. Portable C (may auto-vectorize) is the
 * default. TVC_SIMD_PIXELS opts into target-specific intrinsics; the target CPU,
 * not the OS, selects the ISA. TVC_PORTABLE_PIXELS overrides the opt-in. */
#ifndef TVC_ENC_PIXEL_OPS_H
#define TVC_ENC_PIXEL_OPS_H
#include <stdint.h>
#include <stdlib.h>
#if defined(TVC_SIMD_PIXELS) && !defined(TVC_PORTABLE_PIXELS) && defined(_M_ARM64) && defined(_MSC_VER) && !defined(__clang__)
#include <arm64_neon.h>
#define TVC_ENC_NEON 1
#elif defined(TVC_SIMD_PIXELS) && !defined(TVC_PORTABLE_PIXELS) && defined(__aarch64__) && defined(__ARM_NEON)
#include <arm_neon.h>
#define TVC_ENC_NEON 1
#elif defined(TVC_SIMD_PIXELS) && !defined(TVC_PORTABLE_PIXELS) && (defined(__SSE2__) || defined(_M_X64) || (defined(_M_IX86_FP) && _M_IX86_FP >= 2))
#include <emmintrin.h>
#define TVC_ENC_SSE2 1
#endif

static int enc_sad16_pixels(const uint8_t *a, int as, const uint8_t *b, int bs) {
#if defined(TVC_ENC_NEON)
    uint16x8_t sum = vdupq_n_u16(0);
    for (int y = 0; y < 16; y++)
        sum = vpadalq_u8(sum, vabdq_u8(vld1q_u8(a + y * as), vld1q_u8(b + y * bs)));
    /* Maximum total 16*16*255 = 65280 fits the 16-bit reduction. */
    return vaddvq_u16(sum);
#elif defined(TVC_ENC_SSE2)
    __m128i sum = _mm_setzero_si128();
    for (int y = 0; y < 16; y++)
        sum = _mm_add_epi64(sum, _mm_sad_epu8(_mm_loadu_si128((const __m128i *)(a + y * as)),
                                            _mm_loadu_si128((const __m128i *)(b + y * bs))));
    return _mm_cvtsi128_si32(sum) + _mm_cvtsi128_si32(_mm_srli_si128(sum, 8));
#else
    int sum = 0;
    for (int y = 0; y < 16; y++) for (int x = 0; x < 16; x++)
        sum += abs(a[y * as + x] - b[y * bs + x]);
    return sum;
#endif
}
#endif

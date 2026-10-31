/* Exact encoder motion-vector cost. Compiler intrinsics use the target's
 * baseline bit scan / leading-zero instruction; portable C is the fallback. */
#ifndef TVC_ENC_BIT_OPS_H
#define TVC_ENC_BIT_OPS_H
#include <limits.h>
#if defined(_MSC_VER) && !defined(TVC_PORTABLE_BITS)
#include <intrin.h>
#endif
static double enc_mvbits(int d) {
    unsigned v = d < 0 ? 0u - (unsigned)d : (unsigned)d;
    if (!v) return 1.0; /* Never pass zero to a bit-scan intrinsic. */
    unsigned top;
#if !defined(TVC_PORTABLE_BITS) && (defined(__GNUC__) || defined(__clang__)) && UINT_MAX == 0xffffffffU
    top = 31u - (unsigned)__builtin_clz(v);
#elif !defined(TVC_PORTABLE_BITS) && defined(_MSC_VER) && UINT_MAX == 0xffffffffU
    unsigned long index;
    _BitScanReverse(&index, v);
    top = (unsigned)index;
#else
    top = 0;
    while (v >>= 1) top++;
#endif
    return 2.0 * top + 3.0;
}
#endif

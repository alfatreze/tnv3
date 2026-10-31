#include "bit_ops.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
static void check(int d) {
    double mag = fabs((double)d);
    double expected = mag ? 2.0 * floor(log2(mag)) + 3.0 : 1.0;
    assert(enc_mvbits(d) == expected);
}
int main(void) {
    for (int d = -65535; d <= 65535; d++) check(d);
    check(INT_MIN); check(INT_MIN + 1); check(INT_MAX); check(0);
    for (unsigned bit = 1; bit < sizeof(int) * CHAR_BIT - 1; bit++) {
        int v = (int)(1u << bit);
        check(v - 1); check(v); check(v + 1);
        check(-v - 1); check(-v); check(-v + 1);
    }
    puts("PASS motion bit costs: signed range +/-65535, zero, powers, integer extrema");
    return 0;
}

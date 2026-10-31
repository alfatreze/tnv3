#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include "bpool.h"
typedef struct { unsigned in, out; } Task;
static void calc(void *p) { Task *t = p; t->out = t->in * 17u + 3u; }
int main(int argc, char **argv) {
    EncPool pool;
    if (argc > 1) { assert(!enc_pool_init_n(&pool, atoi(argv[1]))); assert(pool.count == 0); puts("PASS worker startup failure cleanup"); return 0; }
    assert(enc_pool_init(&pool));
    for (unsigned i = 0; i < 100000; i++) {
        Task a = {i, 0}, b = {i + 1, 0};
        enc_pool_submit(&pool, 0, calc, &a); enc_pool_submit(&pool, 1, calc, &b);
        enc_pool_wait(&pool, 0); enc_pool_wait(&pool, 1);
        assert(a.out == a.in * 17u + 3u && b.out == b.in * 17u + 3u);
    }
    enc_pool_destroy(&pool);
    assert(enc_pool_init_n(&pool, 4));
    for (unsigned i = 0; i < 10000; i++) {
        Task jobs[4];
        for (int j = 0; j < 4; j++) { jobs[j] = (Task){i + (unsigned)j, 0}; enc_pool_submit(&pool, j, calc, &jobs[j]); }
        for (int j = 0; j < 4; j++) { enc_pool_wait(&pool, j); assert(jobs[j].out == jobs[j].in * 17u + 3u); }
    }
    enc_pool_destroy(&pool); puts("PASS 240000 tasks with ordered reuse across two and four workers");
    return 0;
}

/* Optional experimental bounded POSIX worker pool. No codec state is shared
 * by this scheduler: each task and its context are owned by the caller. The
 * caller must wait for each submitted task before reusing its slot or destroying
 * the pool. Submit/wait/destroy are called only by the owning thread. */
#ifndef TVC_ENC_BPOOL_H
#define TVC_ENC_BPOOL_H
#define TVC_ENC_BPOOL_CAPACITY 4
#if defined(_WIN32)
#include "bpool_win.h"
#else
#include <pthread.h>
#include <string.h>

typedef struct {
    pthread_t thread;
    pthread_mutex_t lock;
    pthread_cond_t wake, done;
    void (*fn)(void *);
    void *arg;
    int state; /* 0 idle, 1 submitted/running, 2 finished, 3 stopping */
} EncWorker;
typedef struct { EncWorker workers[TVC_ENC_BPOOL_CAPACITY]; int count; } EncPool;

static void *enc_worker_main(void *arg) {
    EncWorker *w = arg;
    pthread_mutex_lock(&w->lock);
    for (;;) {
        while (w->state == 0 || w->state == 2) pthread_cond_wait(&w->wake, &w->lock);
        if (w->state == 3) break;
        void (*fn)(void *) = w->fn; void *task = w->arg;
        pthread_mutex_unlock(&w->lock);
        fn(task);
        pthread_mutex_lock(&w->lock);
        w->state = 2;
        pthread_cond_signal(&w->done);
    }
    pthread_mutex_unlock(&w->lock);
    return NULL;
}
static void enc_pool_destroy(EncPool *p) {
    for (int i = 0; i < p->count; i++) {
        EncWorker *w = &p->workers[i];
        pthread_mutex_lock(&w->lock);
        w->state = 3; pthread_cond_signal(&w->wake);
        pthread_mutex_unlock(&w->lock);
        pthread_join(w->thread, NULL);
        pthread_cond_destroy(&w->done); pthread_cond_destroy(&w->wake);
        pthread_mutex_destroy(&w->lock);
    }
    p->count = 0;
}
static int enc_pool_init_n(EncPool *p, int count) {
    memset(p, 0, sizeof *p);
    if (count < 1 || count > TVC_ENC_BPOOL_CAPACITY) return 0;
    for (int i = 0; i < count; i++) {
        EncWorker *w = &p->workers[i];
        if (pthread_mutex_init(&w->lock, NULL)) goto fail;
        if (pthread_cond_init(&w->wake, NULL)) { pthread_mutex_destroy(&w->lock); goto fail; }
        if (pthread_cond_init(&w->done, NULL)) { pthread_cond_destroy(&w->wake); pthread_mutex_destroy(&w->lock); goto fail; }
        if (pthread_create(&w->thread, NULL, enc_worker_main, w)) {
            pthread_cond_destroy(&w->done); pthread_cond_destroy(&w->wake); pthread_mutex_destroy(&w->lock); goto fail;
        }
        p->count++;
    }
    return 1;
fail:
    enc_pool_destroy(p);
    return 0;
}
static int enc_pool_init(EncPool *p) { return enc_pool_init_n(p, 2); }
static void enc_pool_submit(EncPool *p, int i, void (*fn)(void *), void *arg) {
    EncWorker *w = &p->workers[i];
    pthread_mutex_lock(&w->lock);
    w->fn = fn; w->arg = arg; w->state = 1;
    pthread_cond_signal(&w->wake);
    pthread_mutex_unlock(&w->lock);
}
static void enc_pool_wait(EncPool *p, int i) {
    EncWorker *w = &p->workers[i];
    pthread_mutex_lock(&w->lock);
    while (w->state != 2) pthread_cond_wait(&w->done, &w->lock);
    w->state = 0;
    pthread_mutex_unlock(&w->lock);
}
#endif /* _WIN32 */
#endif /* TVC_ENC_BPOOL_H */

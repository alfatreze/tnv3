/* Experimental Windows worker pool. Caller waits before reusing each slot or
 * destroying the pool. Not enabled in the production encoder. */
#ifndef TVC_ENC_BPOOL_WIN_H
#define TVC_ENC_BPOOL_WIN_H
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0600 /* Vista condition variables and InitializeCriticalSectionEx */
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <process.h>
#include <stdint.h>
#include <string.h>
typedef struct {
    HANDLE thread;
    CRITICAL_SECTION lock;
    CONDITION_VARIABLE wake, done;
    void (*fn)(void *);
    void *arg;
    int state;
} EncWorker;
typedef struct { EncWorker workers[TVC_ENC_BPOOL_CAPACITY]; int count; } EncPool;
static unsigned __stdcall enc_worker_main(void *arg) {
    EncWorker *w = arg;
    EnterCriticalSection(&w->lock);
    for (;;) {
        while (w->state == 0 || w->state == 2)
            SleepConditionVariableCS(&w->wake, &w->lock, INFINITE);
        if (w->state == 3) break;
        void (*fn)(void *) = w->fn; void *task = w->arg;
        LeaveCriticalSection(&w->lock); fn(task); EnterCriticalSection(&w->lock);
        w->state = 2; WakeConditionVariable(&w->done);
    }
    LeaveCriticalSection(&w->lock);
    return 0;
}
static void enc_pool_destroy(EncPool *p) {
    for (int i = 0; i < p->count; i++) {
        EncWorker *w = &p->workers[i];
        EnterCriticalSection(&w->lock); w->state = 3; WakeConditionVariable(&w->wake); LeaveCriticalSection(&w->lock);
        WaitForSingleObject(w->thread, INFINITE); CloseHandle(w->thread); DeleteCriticalSection(&w->lock);
    }
    p->count = 0;
}
static int enc_pool_init_n(EncPool *p, int count) {
    memset(p, 0, sizeof *p);
    if (count < 1 || count > TVC_ENC_BPOOL_CAPACITY) return 0;
    for (int i = 0; i < count; i++) {
        EncWorker *w = &p->workers[i];
        if (!InitializeCriticalSectionEx(&w->lock, 0, 0)) goto fail;
        InitializeConditionVariable(&w->wake); InitializeConditionVariable(&w->done);
        w->thread = (HANDLE)_beginthreadex(NULL, 0, enc_worker_main, w, 0, NULL);
        if (!w->thread) { DeleteCriticalSection(&w->lock); goto fail; }
        p->count++;
    }
    return 1;
fail:
    enc_pool_destroy(p); return 0;
}
static int enc_pool_init(EncPool *p) { return enc_pool_init_n(p, 2); }
static void enc_pool_submit(EncPool *p, int i, void (*fn)(void *), void *arg) {
    EncWorker *w = &p->workers[i];
    EnterCriticalSection(&w->lock); w->fn = fn; w->arg = arg; w->state = 1;
    WakeConditionVariable(&w->wake); LeaveCriticalSection(&w->lock);
}
static void enc_pool_wait(EncPool *p, int i) {
    EncWorker *w = &p->workers[i];
    EnterCriticalSection(&w->lock);
    while (w->state != 2) SleepConditionVariableCS(&w->done, &w->lock, INFINITE);
    w->state = 0; LeaveCriticalSection(&w->lock);
}
#endif

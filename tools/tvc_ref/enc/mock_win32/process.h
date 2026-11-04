#ifndef TVC_TEST_PROCESS_H
#define TVC_TEST_PROCESS_H
#include "windows.h"
typedef struct { unsigned (*fn)(void *); void *arg; } MockStart;
static void *mock_start(void *arg) { MockStart *s=arg; unsigned (*fn)(void *)=s->fn;void *p=s->arg;free(s);fn(p);return NULL; }
static uintptr_t _beginthreadex(void *security, unsigned stack, unsigned (*fn)(void *), void *arg, unsigned flags, unsigned *id) {
    static int attempts;
    (void)security;(void)stack;(void)flags;(void)id;
    const char *fail=getenv("TVC_MOCK_THREAD_FAIL_AT");
    attempts++; if(fail && atoi(fail)==attempts)return 0;
    HANDLE h=malloc(sizeof *h);MockStart *s=malloc(sizeof *s);
    if(!h||!s){free(h);free(s);return 0;}
    s->fn=fn;s->arg=arg;
    if(pthread_create(&h->t,NULL,mock_start,s)){free(h);free(s);return 0;}
    return (uintptr_t)h;
}
#endif

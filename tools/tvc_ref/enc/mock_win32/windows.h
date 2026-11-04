/* Test-only Win32 API model on pthreads; not a Windows SDK or native test. */
#ifndef TVC_TEST_WINDOWS_H
#define TVC_TEST_WINDOWS_H
#include <pthread.h>
#include <stdlib.h>
#include <stdint.h>
#define __stdcall
#define INFINITE 0xffffffffu
typedef pthread_mutex_t CRITICAL_SECTION;
typedef pthread_cond_t CONDITION_VARIABLE;
typedef struct { pthread_t t; } *HANDLE;
static int InitializeCriticalSectionEx(CRITICAL_SECTION *m, unsigned a, unsigned b) { (void)a;(void)b;return pthread_mutex_init(m,NULL)==0; }
static void DeleteCriticalSection(CRITICAL_SECTION *m) { pthread_mutex_destroy(m); }
static void EnterCriticalSection(CRITICAL_SECTION *m) { pthread_mutex_lock(m); }
static void LeaveCriticalSection(CRITICAL_SECTION *m) { pthread_mutex_unlock(m); }
static void InitializeConditionVariable(CONDITION_VARIABLE *c) { pthread_cond_init(c,NULL); }
static int SleepConditionVariableCS(CONDITION_VARIABLE *c, CRITICAL_SECTION *m, unsigned timeout) { (void)timeout;return pthread_cond_wait(c,m)==0; }
static void WakeConditionVariable(CONDITION_VARIABLE *c) { pthread_cond_signal(c); }
static unsigned WaitForSingleObject(HANDLE h, unsigned timeout) { (void)timeout;return (unsigned)pthread_join(h->t,NULL); }
static int CloseHandle(HANDLE h) { free(h);return 1; }
#endif

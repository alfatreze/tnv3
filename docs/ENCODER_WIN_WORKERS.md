# Experimental Windows worker backend (E-043)

The optional worker-pool header selects `bpool_win.h` when `_WIN32` is defined;
otherwise it uses the existing pthread backend. The Windows branch uses a
critical section and condition variables per worker, `_beginthreadex`, a wait,
and handle/critical-section cleanup. It joins already-created workers if a
later startup fails and returns failure so the encoder uses serial fallback.
Each pool slot remains caller-owned: wait before reuse or destruction. This is
experimental and is not linked into the default encoder.

The Windows API design follows Microsoft documentation for [condition variables](https://learn.microsoft.com/en-us/windows/win32/sync/condition-variables),
[SleepConditionVariableCS](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-sleepconditionvariablecs),
and [thread creation and CRT considerations](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createthread).
The wait loop rechecks state after wakes. `_beginthreadex` is used because worker
code calls the C runtime. Actual SDK/compiler/runtime behavior remains unverified.

No Windows SDK, MinGW, Wine, Docker, or native Windows/Linux machine was found
in this environment. `mock_win32/` is a **test-only pthread-backed API model**,
not a Windows SDK or Windows emulator. On macOS it exercises state transitions,
200000 reused tasks, simulated first/second worker creation failure and a bounded
ThreadSanitizer run. The explicit-context encoder compiled against that model
passes its corpus gate, five level/AQ/slice stress cases, forced startup-failure
stream/reconstruction comparisons and a 240-frame level2 film complete-file
comparison against E-022. Evidence: encoder_winmock_gate.json,
encoder_winmock_stress.json, encoder_film_winmock.json and
work/encoder_lab/winmock-tsan.log. This does not prove native Windows ABI,
condition-variable behavior, compiler compatibility, performance or reliability.

Reproduce the model on macOS:

```sh
cc -O2 -pthread -D_WIN32 -I tools/tvc_ref/enc/mock_win32 -o work/encoder_lab/check-bpool-winmock tools/tvc_ref/enc/check_bpool.c
work/encoder_lab/check-bpool-winmock
TVC_MOCK_THREAD_FAIL_AT=1 work/encoder_lab/check-bpool-winmock 2
TVC_MOCK_THREAD_FAIL_AT=2 work/encoder_lab/check-bpool-winmock 2
python3 tools/tvc_ref/enc/make_experiment.py bargs
cc -O3 -pthread -D_WIN32 -I tools/tvc_ref/enc/mock_win32 -I tools/tvc_ref -o work/encoder_lab/encoder-bargs-winmock work/encoder_lab/experiment-bargs.c -lm
TVC_ENCODER_CFLAGS=-O3 TVC_ENCODER_BINARY=work/encoder_lab/encoder-bargs-winmock TVC_GATE_OUTPUT=docs/encoder_winmock_gate.json python3 tools/encoder_lab.py gate
```

Native Windows and Linux builds, full worker gates, robust allocation handling,
long-film tests and platform timings remain required before promotion. No
hardware decoder, FPGA, firmware or Pocket changes are made by this experiment.

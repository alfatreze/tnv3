# Synthetic reference fixtures

These deterministic generators use the C reference `tools/tvc_ref/tvc.h`.
From the codec repository root, the following regenerate the fixture bytes:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/transform.c -lm -o /tmp/tau-transform-reference
/tmp/tau-transform-reference > tau-video/src/tvc/transform-reference.bin
cc -O2 -I tools/tvc_ref tau-video/tests/reference/prediction.c -lm -o /tmp/tau-prediction-reference
/tmp/tau-prediction-reference > tau-video/src/tvc/pred-reference.bin
```

They generate artificial numeric inputs, not media-derived samples. The normal
Rust test/build does not need a C compiler or these generators.

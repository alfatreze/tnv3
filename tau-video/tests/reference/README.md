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

For the motion-compensation fixture:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/inter.c -lm -o /tmp/tau-inter-reference
/tmp/tau-inter-reference > tau-video/src/tvc/inter-reference.bin
```

For the deblocking fixture:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/deblock.c -lm -o /tmp/tau-deblock-reference
/tmp/tau-deblock-reference > tau-video/src/tvc/deblock-reference.bin
```

For the coefficient-coding fixture:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/coeff.c -lm -o /tmp/tau-coeff-reference
/tmp/tau-coeff-reference > tau-video/src/tvc/coeff-reference.bin
```

For motion-vector predictor cases:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/mvp.c -lm -o /tmp/tau-mvp-reference
/tmp/tau-mvp-reference > tau-video/src/tvc/mvp-reference.bin
```

For signed Exp-Golomb coding:

```sh
cc -O2 -I tools/tvc_ref tau-video/tests/reference/seg.c -lm -o /tmp/tau-seg-reference
/tmp/tau-seg-reference > tau-video/src/tvc/seg-reference.bin
```

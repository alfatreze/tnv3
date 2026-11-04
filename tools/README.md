# Tools

Reference encoders, decoders and test generators for TVC, TAC and TNV3.

- `tvc_ref/`, `tnv3_ref/`, `tac_ref/`: the C references (build with `make`) and their Python models and vector generators.
- `tst_ref/`: subtitle (TST) area builder and renderer.
- `codec_study/`: archived AV1 and codec research scripts (some need the Tau V player repository's RTL and cannot run here).
- `encoder_lab.py`, `encoder_guard.py`: the encoder regression gate. `encoder_guard.py` compares against a frozen decoder tag, `tvc-dec-draft5`, that exists only in the Tau V repository; pass a different BASE as the first argument to use a local reference.

The conformance vectors (`*/vectors/`) are not published here because several are derived from film and anime sources; they live in
Tau V (alfatreze/Tau-v) and in the git-ignored `local-media/`. Scripts such as `make_vectors.py`, `run_vectors.py` and `check_roundtrip.py` need
them to be present.

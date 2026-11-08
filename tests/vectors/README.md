# Vectors owned by this repository

`tnv3_15_16/` holds 18 TNV3 files for container drafts 1.5 (baked outline, event bottom bound) and 1.6 (video codec id,
reserved header bytes): 6 valid (`ok*`) and 12 must-reject (`bad*`), each breaking exactly one rule. `expected.txt` lists
name, verdict, SHA-256 and the rule, written from the spec (not from a checker's output).

- Rebuild and check: `make vectors-15-16` (needs `make all`). The generator is `make_vectors_15_16.py`, stdlib only; the
  files are deterministic (a second run gives identical bytes) and use no Tau V data.
- The C checker (`tnv3 check`) and the Rust checker (`tau-codecs-cli check --scope container-preview`, run by
  `tau-video-cli/tests/vectors_15_16.rs`) both give the expected verdict on all 18.
- Tau V may pin these by path and SHA-256 (never copy), as we do with theirs (`tests/pins/`).

# Interop ledger (Tau codecs side)

## Tau V (mailbox at tau-nu/.claude/worktrees/codec-alternatives/docs/interop/to-tnv3/)
| id | what | who answers | status |
|---|---|---|---|
| TV/Q-001 | what the encoder comparison measures | us | ANSWERED, to-tau-v/001 |
| TV/R-002 | CLI encode `--width`/`--level` | owner | DONE 2026-11-08 (default stays 400, level auto) |
| TV/R-003 | adopt container draft 1.5 (outline_w, second plane) | owner | DONE in spec and Rust checker 2026-11-08; C checker and vectors open |
| TV/R-004, R-006 | builder defaults, variant list, centring option | owner | DECIDED: authoring in CLI and crate; not started |
| TV/R-005 | spec text for F-006 (recommended) and F-007 (normative), bound 720 | owner | DONE in spec 6.4/6.5 and Rust check |
| C-1 | draft 1.5 text | Tau V | CLOSED, read in place (`docs/TST_OUTLINE_BAKED_V1_5.md`) |
| C-2, C-3, R-1, R-2 | wide check need, CLI build, vectors, builder behaviour | Tau V | ANSWERED in Tau V 003 |
| TV/Q-1 (their 003) | check run of 800x450 film | us | DONE, to-tau-v/003 (all scopes clean, scoped only) |
| R-3 | Tau V to give `--version --json` of builds they run | Tau V | OPEN |
| Subtitles in v1 | Tau V plays them on hardware | owner | DECIDED yes |
| Pins of Tau V vectors (their 003 R-1) | owner go given 2026-11-08 | owner | DONE as in-place pins: `tests/pins/tau_v_pins.json` + `check_pins.py` (nothing copied) |
| Codec id, extension, licence, MP3 sidecar (owner 2026-11-08) | draft 1.6 codec id byte 22; `.tnv3` final; Apache-2.0 transversal; MP3 parked | owner | DONE here; Tau V to apply licence (to-tau-v/004) |
| TAC decode path (owner 2026-11-08) | C-tool wrapper + `decode-audio`; pure-Rust decoder parked until format closed | owner | DONE; Omega Q-2 (in-process) moot until reconsidered |
| Mux of finished streams (own initiative, follows R-004/R-006) | `mux` CLI command via C tool; remux of the 400x224 film is byte-identical | owner | DONE; authoring and TAC encode still open |
| TAC encode in CLI (owner 2026-11-08) | `encode-audio` command via FFmpeg + `tac enc`; `encode` integration open | owner | PARTLY DONE |

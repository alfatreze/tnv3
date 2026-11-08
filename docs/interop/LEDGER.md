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
| TAC audio inside `encode` (owner 2026-11-08) | `--audio tac` opt-in on both backends; default still MP3 sidecar | owner | DONE; default switch open (owner decision) |
| TAC default audio in `encode` (owner 2026-11-08) | default is now embedded TAC; MP3 pair only on explicit flag | owner | DONE; Tauri app does not use this CLI (still MP3 pair); Omega callers unknown |
| Tauri app on TAC (owner 2026-11-08) | one `.tnv3` with embedded TAC; MP3 UI removed; UI/player not run | owner | DONE in code, UI unverified |
| Audio track selection (owner 2026-11-08) | CLI `--audio-track`, app dropdown; subtitle selection waits for authoring | owner | DONE (audio); subtitle OPEN |
| Subtitle authoring (owner 2026-11-08) | wrapper of Tau V `tst_build.py`: `build-subtitles`, `encode --subtitle-*`, app dropdown; clamp warnings reported | owner | FIRST VERSION DONE; Rust port, ASS styles, presets open |
| TV/005 F-3, 006 (their) | n22 vector pinned; 800 failure is their decoder size limit | Tau V | CLOSED here (to-tau-v/006) |
| Pure-Rust subtitle builder (owner 2026-11-08) | native default; matches Tau V events byte-for-byte | owner | DONE; default font decision, R-4 clamp open |
| R-4 clamp case | what the spec says when the stack does not fit above row 0 | Tau V | OPEN (we follow their tool) |
| Default subtitle font (owner 2026-11-08) | Roboto Regular+Italic bundled in `tau-video/assets/fonts` (OFL); Google Fonts chooser is a future request | owner | DONE; chooser OPEN (future) |
| 800-wide encode and probe (owner 2026-11-08) | real `--width 800` encode valid on both backends; `probe --width` | owner | DONE; device limit 640x400 (Tau V 006) |
| 1.5/1.6 vectors as files (owner 2026-11-08) | 18 files in `tests/vectors/tnv3_15_16`, both checkers agree; Tau V may pin them | owner | DONE (offer to Tau V in the next message) |
| Rust subtitle reader (owner 2026-11-08) | `subtitle_read` + `subtitles` command; reference drawing of 6.3 | owner | DONE; to-tau-v/007 offers the 1.5/1.6 vectors |
| Pure-Rust mux (owner 2026-11-08) | `container::mux_streams`, byte-identical to `tnv3 mux`; CLI/encode/app use it | owner | DONE |
| Encoder logo (owner 2026-11-08) | concept 8 (progress ring, Tau Omega green) chosen and applied | owner | DONE |
| ASS as subtitle intermediate (owner 2026-11-08) | extraction as ASS keeps styles/colours/italics; positioning still ignored | owner | DONE |
| Bundle Roboto Bold (owner 2026-11-08) | Bold and Bold Italic added to the built-in subtitle fonts | owner | DONE |
| Silent test skips (owner 2026-11-08) | guard tests + one conformance environment; TAC audio now cancellable | owner | DONE |
| Reader drawing vs tst_render.py | 266 frames x 3 layers identical (`make compare-render`) | owner | DONE; offer to mention in next message to Tau V |
| Panic-freedom fuzz (audit §7) | mutation tests over readers, muxer, area parser, builder; one panic found and fixed | owner | DONE first level |
| Subtitles in the player (owner 2026-11-08) | overlay from the Rust reader, menu, forced-only when off; real window not driven | owner | DONE in code; open question for Tau V: does the Pocket scale a 400-wide picture? |
| Speed comparison (owner 2026-11-08) | Rust+C = Python+C; Tau V's encode_film.py is slower and 2.9% larger because their tvc_enc.c is older | owner | DONE; tell Tau V their tvc_enc copy is older |
| Dock progress (owner 2026-11-08) | native progress bar during encode, cleared at the end | owner | DONE in code, Dock not seen |
| Progress on the icon (owner 2026-11-08) | segmented amber bar drawn on the Dock icon (video 3/4, audio 1/8, subtitles 1/8, shimmer, end flashes); Dock not seen live | owner | DONE in code |

# Codec audit and correction plan — 2026-11-07

Scope: handoff ([CLI_DEVELOPMENT_STATUS_2026-11-07.md](CLI_DEVELOPMENT_STATUS_2026-11-07.md)), the four normative specs (TVC draft 5, TNV3 draft 1.4, TAC draft 1.2, TST draft 1), the C reference tools, the `tau-video` crate, the CLI, and the test and CI setup. Everything marked **verified** was run today on macOS arm64. Nothing was changed except this file.

## 1. Verdict

The stream has drifted. Since 2026-11-01 the effort has gone into CLI process hardening: cancellation, probe supervision, SHA-256 tool handshakes, exFAT publication fallbacks, cooperative stops and issues 007–020. The CLI is about 6,900 lines against 1,800 lines of C reference codec tools. Meanwhile the things that decide whether the codec ships are open:

- The formats are not frozen and cannot be fully checked as written (section 3).
- Hardware playback is unexplained: choppy playback with Rust output, and a C-encoded file that freezes (issue 003).
- A/V sync of the interim MP3 sidecar path is unmeasured (issues 005/006). TAC is now the primary path (section 6).
- The Rust encoder is far from the C encoder: 148 KB against 51 KB on the same clip (`RUST_C_ENCODER_PARITY.md`).

The decoder-side core is in good shape. TVC decode in Rust matches the C decoder on all local vectors, and the C reference passes its smoke test. The problem is what surrounds it: the conformance evidence is not in the repo, the specs are not self-consistent, and several guarantees are only claimed.

**Recommendation:** freeze CLI feature work and fix bugs only. Spend the next stretch on spec consolidation, the conformance suite and the hardware timing question. Resume the CLI (checker, resume, estimates) only after the error semantics in section 3 exist.

## 2. Verified findings

| # | Finding | Evidence |
|---|---|---|
| V1 | `make all` fails to link on this machine. The Command Line Tools SDK (`MacOSX27.2.sdk`) has a `libSystem.B.tbd` that Xcode's `ld` rejects ("unknown architecture arm64e.x1-macos"). | `make all`. Setting `SDKROOT` to Xcode's `MacOSX.sdk` works around it. |
| V2 | With that workaround, `tests/smoke.py` passes (serial and slice encoder, TAC, TNV3 check). `tau-video` passes 57 lib tests plus its integration suites. The CLI passes 72 unit tests plus its integration suites. | Run today. |
| V3 | `tests/check_manifest.py` fails: 27 of 519 pinned files no longer match `SOURCE_MANIFEST.json` (CHANGELOG.md, PROJECT.md, tvc_enc.c, tnv3.c, `tau-video/src/tvc/stream.rs` and others). `make test` and `ci.yml` run this script first, so that CI job should be red. I could not see CI status. | `python3 tests/check_manifest.py` |
| V4 | About 25 Rust tests exit early and report "ok" unless `TAU_VIDEO_VECTOR_DIR`, `TAU_TVC_VECTOR_DIR`, `TAU_TVC_DECODER`, `TAU_TNV3_TOOL` or `TAU_TEST_TNV3` are set. No workflow in `.github/` sets them. In a default `cargo test` or in CI, all vector, C-decoder and checker comparisons are skipped silently. They do pass when pointed at `local-media/`. | `grep` of the tests and workflows. Run with the env vars set. |
| V5 | The conformance assets the specs cite are not in this repo. The vectors (tvc 21 valid + 3 negative, tnv3 9 + 21, tac t01–t08) sit in git-ignored `local-media/`. The independent Python decoders (`tvc_dec.py`, `tac_dec.py`, `tac_fx.py`, `tnv3_demux.py`), `make_vectors.py`, `mutants.py` and `tst_ref/` exist only in the sibling `tau-nu` tree. | `git ls-files`, `find`. |
| V6 | The Rust decoder does not enforce every level rule: slice minimum for level 3 and the vector-range limit are absent. `stream.rs` says so itself. There is no Rust TAC decoder and no Rust subtitle support. | `stream.rs` docs, source search, `RUST_ROADMAP.md`. |
| V7 | A flaky test is unexplained (issue 019: output filename published although the progress stream broke). It is the publication-ordering guarantee, so it is a correctness question, not only test noise. | Issue 019. |

## 3. Format-definition gaps

These are the "format not fully defined" items. They must be settled before a full checker (issues 017/020) or a freeze is meaningful.

**F1. Spec hygiene.**
- TVC is titled "draft 2" but describes draft 5.
- TVC section 7b is numbered 1–3, 10–13, 4–9, 14–15, 17, 16, 19, 18.
- TAC's header says "not reviewed by an independent implementation", while its 7b says an independent decoder matches.
- TNV3 says "15 must-reject files"; the vector set has 21.
- TNV3 cites `tools/tst_ref/` and `tools/tnv3_ref/py/`, neither of which is in this repo.
- TST says "Bitmap events: later draft."
- There is no changelog per spec and no single "current normative revision".

**F2. No versioning or extension model.**
- TNV3 `version` is fixed at 0.
- TVC features are toggled per picture (`qpel`, `t4ok`, `sliced`) with no stream-level profile.
- TVC 7b.14 says a conforming encoder uses the same `qpel` value throughout, but nothing enforces or checks it.
- "Reserved must be 0" means every later extension breaks old decoders.
- No criteria say what counts as "final and frozen", which is the condition under which resume was parked.

**F3. "Valid stream" is not defined at the bit level.**
- The range decoder returns 0 past the end of data. There is no flush or termination rule, no trailing-byte rule and no check that a slice consumed its length.
- TVC section 9 says a corrupt picture is "undefined behaviour", while TNV3 section 7 says demuxers "must reject" invalid files.
- Several rejection cases are unspecified: EG `k > 24`; a slice-length table that does not sum to the picture length; `rows = 0`; reserved flag bits 4–7; vectors over the level limit (spec only says the decoder reports failure); picture type 3; a B picture whose anchors are missing.
- Without this list, "decode succeeded" cannot mean "conformant", which is what issue 020 found.

**F4. No decoder cost or buffering model.**
- Levels bound bytes, macroblocks per second and slices. They do not bound entropy bins per picture or per second, the intra share, the B-run length, or the read-ahead buffer (TNV3 section 9 "Open").
- Issue 003 fits an unbounded worst case: a 93.8%-intra stream at 24 fps stutters, and a C stream freezes after a long run.
- Until the player has a cost model, "level 3 conformant" does not mean "plays".

**F5. Audio contract is split.**
- TNV3 defines audio as TAC with the TAC sample counter as master clock.
- The shipping path is an MP3 sidecar outside the container, with unmeasured delay (issue 006).
- TAC has no Rust decoder, no hardware decoder, and its own open items (short-window grouping, lower rates).
- A normative sync definition for the sidecar is needed, or the Pocket pair must be declared interim.

**F6. Subtitle scope.** TST is defined and checked by the C tool only. Decide whether it is in v1.

## 4. Test gaps

- **T1.** The Rust decoder is checked only against the C decoder (same author, same design). The independent Python decoders exist but are not part of the Rust gate.
- **T2.** No negative vectors exist for the F3 cases (TVC has 3 negatives, all level-related). TAC has no negative vectors for its 7b.8 invalid-stream list.
- **T3.** No fuzzing or mutation corpus for the Rust decoder. Issue 020 states panic safety, CPU time and memory are not established.
- **T4.** The encoder is evaluated on short clips (4–12 cases). There are no long-film runs, no equal-quality rate curves, and no hardware decode-time data per picture.
- **T5.** Linux and Windows runtime are untested. CI only proves builds. Python and process-tree cancellation are unsupported.
- **T6.** The Rust encoder's compliance with level 3 (two slices minimum) is not covered by a test I could find. Verify it as part of the work in section 5.

## 5. Correction plan

Each phase has an exit criterion. Sizes: S ≈ under a day, M ≈ a few days, L ≈ a week or more.

**Phase 0 — repo hygiene (S)**
1. Document the macOS SDK workaround and add an `SDKROOT` fallback to the Makefile. Verify the Ubuntu and macOS CI jobs.
2. Regenerate `SOURCE_MANIFEST.json`, or remove it from `make test` until it is automated.
3. Make vector-dependent tests fail loudly. Either require the env vars under a `conformance` feature, or print and count skips. CI must set them.
4. Fix or explain issue 019 by recording stderr and event order when it recurs.

*Exit:* a clean checkout runs `make test` and `cargo test` green in CI, and the number of skipped comparisons is visible.

**Phase 1 — spec consolidation, "v0 release candidate" (M)**
1. Relabel and renumber TVC, TAC and TNV3, and add a per-spec changelog.
2. Add a normative "Errors and termination" section to TVC: the exact reject list from F3, a flush and trailing-byte rule, and `k` and vector bounds.
3. Add a feature and profile model: a stream-level flag set (or drop the per-picture flexibility), a version policy and an extension rule.
4. Write the freeze checklist: what must be true before "final and frozen".
5. Record the audio contract (TAC primary; MP3 sidecar optional and outside the core spec) and decide the subtitle scope (F6).
6. Decide how a future video codec id is reserved in TNV3 (section 7B.1).

*Exit:* each spec has one current revision, and every "must reject" has a testable condition.

**Phase 2 — conformance suite in the repo (M–L)**
1. Move vectors, `make_vectors.py`, the Python independent decoders and `mutants.py` from `tau-nu` into this repo, with provenance. Keep generated media small.
2. Add negative vectors for every Phase 1 reject rule, for TVC, TAC and TNV3.
3. Run C, Rust and Python decoders against every vector in CI (three-way agreement).
4. Add a bounded fuzz and mutation run for the Rust decoder: no panic, bounded time and memory.

*Exit:* all three decoders agree on the full set, and the fuzz run finds no panic.

**Phase 3 — finish the Rust checker (M, after Phases 1–2)**
Implement issues 017 and 020 against the Phase 1 error list: typed invalid, unsupported, I/O and limit outcomes with offsets, and the missing level rules (slice minimum, vector range). Only then enable an unqualified `check`.

**Phase 4 — playback and timing evidence (M–L, needs hardware)**
1. Instrument the player (issue 003): decode time per picture, bins per picture, dropped frames.
2. Reproduce the freeze and the pause behaviour.
3. Measure MP3 delay on the real decoder (issue 006).
4. Use the data to add a decoder cost bound to the levels (F4).

*Exit:* the Pocket playback issues are closed with data, and the level table has a cost limit.

**Phase 5 — encoder decision (M)**
Choose between a safe-Rust binding to the C encoder and a full Rust port. Rust is currently 2.9× the C size at similar PSNR. Measure equal-quality rate curves on long films first. Stop adding Rust encoder experiments in the meantime.

**Phase 1b — architecture seams (M, after Phase 0, parallel with Phase 2)**
Implement section 7A (audio interface, `mp3-sidecar` feature) and the section 7B seams (codec id enum, decoder/encoder/level traits, backend description) with TVC as the only codec. Exit: the build without `mp3-sidecar` has no MP3 code, and all existing tests pass unchanged with and without the feature.

**Phase 6 — CLI resume (deferred)**
Take up resume, full-pair estimates and Python cancellation only after the format freeze. Do not copy the incomplete `work/` drafts without re-review.

**Freeze now (bug fixes only):** cooperative stop, SIGINT and stdin-EOF cancellation, tool-handshake hashing, probe sample estimates, schema-1 events, the exFAT non-atomic fallback.

## 6. Owner decisions

Decided 2026-11-07:
1. **Audio:** TAC is the primary audio path. The MP3 sidecar stays as an advanced, flag-based option that can be dropped from a build (section 7).
2. **Future video codecs:** keep the option of an AV1 specialised variant and MPEG-4 (from the earlier Tau V builds). Nothing is implemented now; the architecture must make adding them modular (section 7).

Settled by the interop documents (2026-11-08; Omega's `TAU_VIDEO_CRATE_REQUIREMENTS.md` revision 2026-11-05 and `VIDEO_SYNC_PLAN.md`):
3. **Rust encoder:** not a product goal. The C encoder is the reference, wrapped by the CLI; Omega drives the CLI as a sidecar. Stop Rust encoder experiments (Phase 5 reduces to maintaining the C wrapper).
4. **Consumer needs that change this plan:** Omega needs a Rust TAC decoder in-process for its player (not only for the checker), never-panic decoding (fuzzing, Phase 2), pinned vectors with SHA-256 and a mutation test per checker rule (Phase 2), and no LGPL MPEG-4 material in the crate (applies to section 7B: future codecs go in separate, optional, feature-gated code). Omega defers subtitles to a later release.

Still for the owner to decide (Omega's questions are in its own documents; no reply has been sent to Omega):
5. **Subtitles in v1: DECIDED 2026-11-08, yes.** Container draft 1.5 (baked outline) adopted into `TNV3_CONTAINER_V0.md` and the Rust subtitle checker (done, tested; checker accepts Tau V's real 4-size film). Still to do: C reference checker `tools/tnv3_ref/tnv3.c` (still 1.4), 1.5 vectors with SHA-256, and authoring (next item).
5b. **Authoring: DECIDED 2026-11-08.** Subtitle authoring lives in the CLI and the crate, for built-in subtitles and for external ones (an imported SRT/ASS or a prebuilt area). Not started: the reference is Tau V's `tools/tst_ref/tst_build.py` (915 lines, fonts, wrapping, outline bake, F-006 centring, variant list 32x44/40x54/48x64/56x76 as default). Needs a design: font source and licence, Rust rasteriser, and whether to port or call the Python tool first.
5c. **CLI `--width` and `--level`: DONE 2026-11-08.** Default stays 400 (box 400x360), level automatic; `--width` up to 800 (box 800x720) and `--level 0..3` are opt-in, so 800p feasibility can be tested without changing defaults. Not yet applied to `probe` estimates.
5d. **Area size:** no format limit beyond 32 bits; the 1 MB limit is documented as Tau V's player profile (spec 6.1).
5e. **TAC decoding: C-tool wrapper, DECIDED 2026-11-08; pure-Rust TAC decoder PARKED until the TNV3 format is closed (then reconsidered).** Reason: quicker build, less duplication, less testing. Done: `tau_video::c_audio` (feature `c-audio-decoder`) and `tau-codecs-cli decode-audio IN.tnv3 OUT.wav [--model integer|reference]` run `tnv3 demux` then `tac_fx dec` (integer model, hardware oracle) or `tac dec` (double reference). Verified end to end on Tau V's 23-minute film: integer vs reference max diff 1, RMS 0.220 (spec 0.222); both C decoders reproduce the pinned vectors t01-t08 (`tests/pins/check_tac_vectors.py`; t08 integer max 3, RMS 0.107, as in the spec). Limits: this is the C reference, not an independent decoder; the output JSON says `independent_decoder: false`; no cancellation or timeout; needs the C tools at run time (Omega's in-process requirement D3 is therefore NOT met until the Rust decoder is built).
5f. **C reference checker `tnv3` updated to draft 1.5/1.6 (2026-11-08):** outline plane, dilation check, 720 bound, codec id. It accepts both of Tau V's films (an implementation independent of the Rust checker).
5g. **Hygiene and conformance run: DONE 2026-11-08.** `SOURCE_MANIFEST.json` re-pinned (31 files had drifted; provenance kept, `repinned` noted) so `tests/check_manifest.py` passes; `make all` no longer needs `SDKROOT` on macOS with Xcode installed; `tau-video-cli/target/` ignored. Tau V's TVC (25) and TNV3 (31) vector sets are pinned in place, and `make rust-conformance` runs the pin check, the TAC vector check and the Rust test suite with the variables set that otherwise make the reference tests return early (116 pass, 0 fail). The C checker and the Rust container-preview check agree on all 30 TNV3 vectors (21 must-reject, 9 valid). Still open: the plain `cargo test` / `make rust-test` still skips those reference tests silently without the variables; 1.5/1.6-specific must-reject vectors exist only as my mutation tests, not as pinned files.
5h. **`mux` command (C-tool wrapper): DONE 2026-11-08.** `tau-codecs-cli mux VIDEO.tvc OUT.tnv3 [--audio A.tac] [--subtitles AREA.bin] [--crc] [--level] [--rate N/D] [--trim-audio] [--audio-shift N]` wraps `tnv3 mux` (crate feature `c-mux`, module `tau_video::c_mux`); no muxing logic in Rust, output never overwritten (workspace plus no-replace hard link), exit codes 2/3/1 as for `decode-audio`. Verified on Tau V's 400x224 film: demux into TVC, TAC and subtitle area, then remux with `--crc` gives a byte-identical file (same SHA-256) that the C checker calls valid. Tests use a fake tool (wiring, errors, no overwrite) and no test runs the real tool automatically. It joins finished streams only: it does not encode TAC, does not build subtitle areas, and does not yet feed `encode`. Next: TAC encode in the CLI, subtitle authoring, then `encode` producing audio and subtitles in one run.
5i. **`encode-audio` command: DONE 2026-11-08.** `tau-codecs-cli encode-audio INPUT OUTPUT.tac [--bitrate 16..512, default 96] [--channels 1|2, default 2]` runs FFmpeg (first audio stream to 48 kHz 16-bit PCM) and then `tac enc` (module `tau_video::c_audio::encode_audio`, same feature as `decode-audio`); no encoding in Rust, no overwrite, same exit codes. Checked on a 20 s stereo clip (96 kbit/s: 226,877 bytes, 939 frames; 64 kbit/s mono also works) and decoded back with `tac dec` to the same sample count. Tests use fake tools only. Not done: `encode` does not yet call it (a full encode still produces video only or the MP3 sidecar), the TAC bit-rate default of 96 is the C tool's, and sources with more than two channels are downmixed by FFmpeg without a choice of matrix.
5j. **TAC audio inside `encode`: DONE 2026-11-08, opt-in.** `encode ... --audio tac [--audio-bitrate 16..512, default 96] [--tac-tool PATH]` now produces one `.tnv3` with embedded TAC audio, on both backends (Python/C and `--rust-c-orchestrator`). After the video is staged the CLI demuxes it, encodes the source audio with FFmpeg and `tac enc` fitted to the video duration (padded or cut), muxes with `tnv3 mux` (CRC flag kept, `-T`) and writes the result back into the staged payload, so the usual atomic no-replace publication is unchanged. `--audio none|mp3-sidecar` select the other modes; `--audio` cannot be combined with `--video-only` or `--profile`. Checked on an 8 s synthetic clip: `audio=1`, 384,000 samples (exactly 8 s), C checker `valid`, `decode-audio` decodes it, no staging left. **Default is unchanged (MP3 sidecar pair)** because the Tauri app and Omega rely on that contract and its two-file events; switching the default to TAC is an owner decision and a one-line change. Limits: the audio step is not cancellable once started (a stop requested earlier is honoured); stereo only; audio is taken from the first audio stream, with no sync offset option; only an 8 s clip was run, not a full-length film.
5k. **TAC is the default audio of `encode`: DONE 2026-11-08 (owner decision).** A plain `encode IN OUT.tnv3` now embeds TAC audio at 96 kbit/s; the old MP3 sidecar pair needs `--audio mp3-sidecar` or `--profile pocket-sidecar@1`, and `--video-only` / `--audio none` write no audio. This is a **behaviour change for callers of the old default** (the Tauri app, Omega): they now get one file instead of `.tnv3` + `.mp3`, and `effective_settings.audio.format` is `tac-embedded`. Callers that need the pair must pass the profile explicitly. Checked on a 4 s clip in all three modes. The Tauri app (`tau-encoder`) does not call this CLI (it runs FFmpeg and its own MP3 code directly, still writing `.tnv3` + `.mp3`), so it is unaffected and still on the MP3 pair; moving it to TAC is separate work.
5l. **Tauri app moved to embedded TAC: DONE 2026-11-08, partly verified.** `tau-encoder` now writes one `.tnv3` with TAC audio (FFmpeg to WAV, `tac enc` 96 kbit/s, `tnv3 demux` + `tnv3 mux -a -T`, CRC flag kept; the Stop button can kill these steps). New setting `tacTool` (Dependency settings, discovery, bundle script now compiles `tac`); `AudioFormat::Mp3` stays in the backend as the parked legacy path (used by the existing fake-tool tests), but no UI offers it. The player decodes embedded audio to a temporary WAV through `tau_video::c_audio` when a file is opened. UI text updated; the MP3 delay checkbox removed. Verified: app `cargo test` passes (33) with the real tools set, including two new tests (a 2 s clip gives one valid file, 48 frames, 96,000 audio samples, no `.mp3`; missing TAC tool fails before publication). Not verified: the app UI itself (no `svelte-check`/`vite build`: `node_modules` is absent; nothing was run in the window), the player's embedded-audio path, the Python-script workflow and the experimental Rust encoder with TAC, a full-length film, and the bundled-app build.
5m. **Audio track selection: DONE 2026-11-08 (audio only).** CLI: `encode --audio-track N` and `encode-audio --track N` (zero-based among the source's audio streams, 0 to 127, default 0; refused when no audio is written); `probe --json` now lists each audio stream's `language` and `title`. App: when a source has more than one audio track, a dropdown (language, title, codec, channels) appears after choosing the file, and the choice is used for the source check and the encode (`list_audio_tracks` command via FFprobe; if FFprobe fails the app silently uses track 0). Verified with a file holding a 440 Hz and a 1000 Hz track: CLI and app tests decode the result and measure the tone of the chosen track (track 0 about 440 Hz, track 1 about 1000 Hz); a nonexistent track fails with no output. The dropdown itself was only checked to compile (`svelte-check` clean), not clicked. **Subtitle track selection is not done:** there is no subtitle authoring yet (the TST area builder), so there is nothing to feed a chosen subtitle stream into; it belongs to the subtitle authoring item and needs the font/render decisions listed there. Also: the selection is a plain index, so for a source whose audio order differs between files it is per file, not remembered.
5n. **Subtitle authoring, first version (wrapper): DONE 2026-11-08.** Decision (mine, to minimise effort, in line with the C-wrapper choices): wrap Tau V's reference builder `tools/tst_ref/tst_build.py` instead of porting it; it is found by `--builder` / `--subtitle-builder`, `TAU_TST_BUILD`, or `TAU_V_ROOT/tools/tst_ref/tst_build.py`, never copied (pin policy). It needs Python 3 and Pillow, and a font: `--font`, else Tau V's `fonts/Roboto-Regular.ttf` next to the builder (OFL, theirs). Crate `tau_video::c_subtitles` (feature `c-subtitles`): `build_area`, `extract_text_subtitle` (FFmpeg to SRT; image subtitles are refused with a clear message). CLI: `build-subtitles OUT.bin --video WxH (--track FILE[:lang[:default][:forced]] | --from-source IN --source-track N[:...])... [--font] [--sizes] [--fps] [--styles] [--outline-width] [--builder-arg]...`, and `encode --subtitle-file/--subtitle-track/--subtitle-font/-sizes/-outline/-styles/-builder-arg/-builder` which build the area from the staged video's size and frame rate and mux it with the audio in one pass (`--audio none` works). Builder warnings, including the known "does not fit above the stack; clamped" placement case, are returned (JSON `warnings`, stderr in `encode`), not hidden; nothing was ported, so the clamp defect is still Tau V's. App: a Subtitles dropdown (none by default; image subtitles greyed out), a Subtitle builder path in Settings, and warnings in the result; only with the embedded TAC output. Verified on real files: a source with two audio and two subtitle streams encoded with audio track 1 and subtitle track 1 (language spa) gives `audio=1 subs=1`, valid by the C checker and the Rust `subtitle-structure` check; two tracks at once and no audio also work; a nonexistent subtitle track fails with no output. Tests: CLI 129, app 37, with fake-Python tests for the wrapper. Not done: ASS styling when extracting from a source (SRT only, so style and position are lost), per-track font/style choices in the app, image subtitle OCR, any player-side subtitle display in the app, size/outline presets in the app (fixed 16x24,20x28,26x36), a long film, and the spec-level questions on 800x720 placement for a 400-wide picture (the area is built for the real picture size).
5o. **Owner decision 2026-11-08, corrected: the subtitle builder is to be written entirely in Rust (no Python, no Pillow).** I had read "keep wrapper" as keeping the Python wrapper; the owner meant a Rust-native builder, as with the original decision ("authoring in the CLI and crate"). The wrapper of 5n is therefore a stop-gap to be replaced; the Rust builder is in progress.
5p. **Pure-Rust subtitle builder: DONE 2026-11-08 (replaces the Python wrapper as the default).** `tau_video::subtitle` (feature `subtitle-builder`; deps `ab_glyph`, `unicode-normalization`, `regex`) is a port of `tst_build.py`; CLI `build-subtitles` and `encode --subtitle-*` and the app use it by default and need no Python or Pillow (the Python wrapper of 5n remains only when `--builder`/`--subtitle-builder`/`TAU_TST_BUILD` is given, for comparison). Compared with Tau V's tool on their three samples: event lists byte-identical, font sizes and all glyph advances identical, areas identical in size; glyph pixels differ by at most one grey level (mean 0.008 of 3). Font: no default is bundled: `--font`, `TAU_SUBTITLE_FONT`, or `TAU_V_ROOT/tools/tst_ref/fonts/Roboto-Regular.ttf`; **owner decision open: bundle an OFL font (Roboto, already used by Tau V) in this repo or keep requiring one**. App: the "Subtitle builder" setting became "Subtitle font". Tests: crate 3 integration + 8 unit tests (integration need `TAU_TEST_FONT`, set by `make rust-conformance`), CLI 129, app 37 including a real-media subtitle test; the C checker and the Rust `subtitle-structure` check accept an encoded film. Known gaps: HTML named entities limited to a short list, character category dropping approximate, ASS styling lost when extracting from a source file (SRT), no per-track fonts in the app, the clamp case is reported but still clamped (Tau V's open R-4). Also this session: Tau V's new vector `n22_video_codec` and changed `expected.txt` re-pinned (their tree was uncommitted), both checkers reject n22; reply 006 sent.
5q. **Default subtitle font bundled: DONE 2026-11-08 (owner: "Bundle").** Roboto Regular and Italic (OFL 1.1, copies of Tau V's files, same SHA-256) are built into the crate (`tau-video/assets/fonts/`, `subtitle::default_fonts()`); the CLI, the crate and the app use them when no font is given (`--font` and `TAU_SUBTITLE_FONT` still override; the app's Subtitle font setting is optional). OFL text in `LICENSES/OFL-Roboto.txt`, NOTICE updated. Binary size grows by about 325 kB. **Future (owner request): a font chooser backed by Google Fonts.** Not started; open questions for it: network access from the app and CLI (the encoder is offline today), where downloaded fonts are cached, licence display (fonts there are OFL/Apache/UFL, each with its own terms), whether a font family's bold and italic faces are fetched together, and an API key for the Google Fonts developer API (or the unauthenticated CSS endpoint); the builder already accepts any `--font [NAME[:STYLE]=]FILE.ttf[@SCALE]`, so the chooser only has to produce files.
5r. **Real 800-wide encode and width-aware probe: DONE 2026-11-08.** A real 1920x1080 source encoded with `--width 800` on both backends (Python/C and `--rust-c-orchestrator`) gives an 800x450, level 2, 24 fps file with embedded TAC audio, valid by the C checker, same size from both backends (501,376 bytes for 6 s of test pattern). Note the Tau V hardware limit from their 006: today's decoder build only decodes up to 640x400 pictures, so 800-wide files are for format and encoder testing, not for the device. `probe` now takes `--width 16..800` (even): the geometry, level and sample estimate use that profile (`pocket-default-reference-800x720-24fps` and so on; the default stays 400x360, JSON name unchanged), and its safety caps on FFmpeg output were raised for 800x720 frames. Not done: the probe's sample estimate is video only (unchanged), and no film-length 800 encode was run.
5s. **New Tau Encoder logo: DONE 2026-11-08.** The encoder had reused the green pulse icon of Tau Omega. New logo: an amber tau (film-strip bar over a stem with three shrinking dashes) on a dark navy tile, source `tau-encoder/branding/tau-encoder-logo.svg`; icons regenerated with `cargo tauri icon` (`32x32`, `128x128`, `128x128@2x`, `icon.icns`, `icon.ico`) and the sidebar mark uses the SVG. Checked at 128, 64, 32 and 16 px (still readable) and `svelte-check` and `vite build` pass; the running app window and the macOS Dock icon were not viewed, and the app's mint UI accent was left unchanged.
5t. **Draft 1.5/1.6 vectors as files: DONE 2026-11-08.** `tests/vectors/tnv3_15_16/` (18 files, 76 kB, deterministic, generated by `tests/vectors/make_vectors_15_16.py` from a synthetic video through our own `tvc_enc` and `tnv3`, so no Tau V data): valid outline widths 0, 1+2 on two sizes and 4, event bottoms exactly on row 720, codec id 0; must-reject outline width 5, plane not equal to the dilation, padding bit in an outline row, variant reserved byte, missing plane, table one byte short, event below the grid (one line and with a break), codec id 1 and 255, header byte 23, codec id 1 without subtitles. Expected verdicts are written from the spec; the C checker agrees on all 18 (the generator's `--verify`), and the Rust checker agrees (`tau-video-cli/tests/vectors_15_16.rs`, which fails rather than skips and checks every SHA-256). `make vectors-15-16` runs all of it. The padding vector is reported by both checkers under the dilation message, not a separate padding message; the rejection is correct.
5u. **Rust subtitle reader: DONE 2026-11-08.** `tau_video::subtitle_read` (always built, no extra dependencies): `parse_subtitle_area` (runs the structural validator first, every read bounds-checked), `Tnv3Reader::read_subtitle_area`, plain data types for tracks, palette, variants, events (with glyph references, colour switches and breaks) and glyphs (fill and outline planes), and `Variant::render(p)` which draws section 6.3 (larger alpha wins, later wins on equal alpha; outline layer as the maximum of the planes, clipped to 800x720). CLI `subtitles FILE.tnv3 [--json] [--events] [--render SECONDS OUT.ppm] [--track N] [--variant N]`. Verified: unit tests on a hand-built area (fields, drawing rules, every cut and every flipped byte is an error or an accepted area, never a panic); the 6 valid vectors are read with their outline widths and the 12 bad ones refused; a builder round trip (events, glyphs, palette, language, SDH and switch counts equal the builder's report; text and outline appear only while an event is active); and a real encoded film drawn at 1.5 s shows "Hello world / second line here" with the italic word (viewed). Tests: crate 137 with all features, CLI 134. Not done: comparison pixel by pixel with Tau V's `tst_render.py`, the player parameters of 6.4 (colours, box, offset, delay), and muxing audio and subtitles in pure Rust (still the C `tnv3 mux` through the wrapper).
6. **Freeze order and criteria** (Omega's Q2). Proposal: container, TVC, TAC, TST last; criteria come from Phase 1.
7. **Poster/index format.** Proposal: none in TNV3; host-side only.
8. **Conformance assets: DECIDED 2026-11-08 (go given), partly done.** Tau V's subtitle material is pinned in place by SHA-256 (`tests/pins/tau_v_pins.json`, verified by `tests/pins/check_pins.py`, missing files fail rather than skip). Not yet pinned: the TNV3/TVC/TAC vectors and Python references that Omega expects (Phase 2), and no assets are copied into this repo.
9. **MP3 sidecar: PARKED 2026-11-08.** Not needed for TNV3 (TAC is the audio path); may return if an MP4 or AV1 alternative build is revived. The CLI's `pocket-sidecar@1` profile is still the default encode output only because the CLI cannot write TAC yet; no new work goes into it and it is to be moved behind the `mp3-sidecar` Cargo feature, off by default, when TAC encode lands in the CLI. Not yet gated in code.
10. **Codec id: DECIDED and DONE 2026-11-08.** Container draft 1.6: header byte 22 = `video_codec` (0 = TVC, 1..255 reserved, readers reject with `E-TNV3-032`); byte 23 stays reserved. Existing files are unchanged and the version stays 0. Applied to the spec, the Rust reader, the C checker (source only, not rebuilt against vectors) and a test.
11. **`.tnv3` extension: FINAL (2026-11-08). Crate licence: Apache-2.0, transversal across Tau V and Tau codecs (2026-11-08).** Applied here: `LICENSE` is Apache-2.0, upstream HarpMudd MIT notice kept at `LICENSES/MIT-HarpMudd.txt`, `NOTICE.md` updated, Cargo licence fields set. Tau V's own repository must make the same change; third-party components keep their own licences (Helix RPSL 1.0, OFL).

## 7. Architecture requirements (design only, no implementation now)

**Current state (verified by code search).**
- The MP3 sidecar is not isolated. `tau-video-cli/src/main.rs` has 77 MP3/sidecar references, the CLI has no Cargo features, and the profile is the CLI default. The crate has an `mp3-cli` feature, but only for the separate `tau-mp3` binary.
- TNV3 has no video-codec identifier. TVC is implied. The header has reserved space (bytes 22–23, 52–59) and a fixed `version = 0`. Adding a codec id later is a format change with a version bump unless the field is reserved now.
- Rust codec code lives in one crate with TVC hard-wired into the container reader, the stream decoder and the encoder.

**A. Audio: TAC primary, MP3 optional**
1. Define an internal audio-track interface (encode to a stream of packets plus timing metadata). TAC and MP3 are two implementations. The CLI workflow talks to the interface, not to MP3 directly.
2. Move all MP3/sidecar code (FFmpeg MP3 encode, pair publication, `pocket-sidecar` profile, delay options, `tau-mp3`) behind one Cargo feature, for example `mp3-sidecar`, default off or default on by your choice. Building without it must compile with no MP3 references and no behaviour change for TAC output.
3. The same flag removes the sidecar CLI options, JSON event fields and tests. `handshake --json` capabilities report whether it is compiled in.
4. The default profile becomes TNV3 with embedded TAC. The MP3 pair is selected explicitly, as an advanced profile.
5. Prerequisite for the default switch: a Rust TAC encoder or a supervised call to the C `tac` tool, plus the TAC checks of Phase 2. The Rust TAC decoder is needed for the checker, not for encoding.
6. Document the sidecar sync definition (issue 006) inside the feature only; the core spec stays TAC-only.

**B. Video: room for AV1 and MPEG-4 later**
1. **Codec identity.** Reserve a video codec id in the next TNV3 revision, with TVC = 0. Until the format is revised, keep the id as an internal enum in the Rust container reader so no code assumes "video means TVC". Decide in Phase 1 whether to spend a header reserved field now or wait for a version bump.
2. **Trait seams.** Introduce the interfaces in `tau-video` and the CLI before any new codec exists, with TVC as the only implementation:
   - `VideoDecoder`: feed a picture, get frames in display order.
   - `VideoEncoder`: frames in, pictures out, with a codec-specific rate/level object.
   - `LevelProfile`: limits and checks per codec (TVC levels become one implementation).
   - A codec registry keyed by the id, each codec behind its own Cargo feature.
3. **Container independence.** The TNV3 reader should expose pictures and timing, not TVC syntax. Container-level checks stay in the container module; codec checks (slice minimum, vector limits, payload validity) move behind the codec interface so the checker can call the right one.
4. **Encoder workflow.** The CLI's segmenting, rate-window and FFmpeg ingest steps currently assume the C `tvc_enc` tool and TVC level names. Put the codec behind a "backend" description (tool or library, supported levels, options) so a different backend can be selected without changing publication, staging or events.
5. **Spec layout.** Keep one normative document per codec (`TVC_…`, later `TAV1_…`, `TMP4_…`), one container spec, one audio spec. Shared pieces (range coder, level concept) should be referenced, not copied.
6. **Hardware reality.** AV1 and MPEG-4 were hardware decoder studies on Tau V (`CODEC_AV1_*`, `ENCODER_DAV1D.md`). A codec only enters the container once its player-side cost fits the decoder cost model from Phase 4. The id reservation costs nothing; the codec does not enter the format until then.
7. **Rule for now:** no code for AV1 or MPEG-4 in this repo. The only work is the seams above, each verified by the existing TVC tests passing unchanged.

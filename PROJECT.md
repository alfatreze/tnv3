# TNV3 codec tools

An alpha host toolkit extracted from Tau V. It contains TVC video, TAC audio and
TNV3 container reference tools and a partial safe Rust `tau-video` crate. The
crate reads TNV3 structure and decodes TVC video; it is not yet a complete
container checker. The C encoder remains the reference, while the pure-Rust
encoder is experimental. Upstream provenance and MIT notices are retained in
`NOTICE.md` and `LICENSE`.

## Current development checkpoint — 2026-11-07

The current CLI and Rust codec status, tested scope, local test-build age and remaining work are recorded in [CLI development status](docs/CLI_DEVELOPMENT_STATUS_2026-11-07.md). Unix SIGINT is supported for the native Rust+C CLI, the source probe offers optional video-only sample projections, and explicit tool version responses and a sampled quick check are available. These are host-scoped features; the full checker, Python/process-tree cancellation, full-pair estimates and tool compatibility remain open. Deterministic resume is deferred until the final format is complete and frozen. Older dated milestone notes below are historical where superseded by this checkpoint.

The default serial encoder and opt-in parallel-slice encoder have passed host
checks on macOS ARM64, and the standalone smoke passes on GitHub-hosted Ubuntu
and macOS runners. Parallel-slice output is identical to serial for the
same slice layout in the tested cases; it remains experimental. Existing
format and decoder evidence is described in `VALIDATION.md`. The hardware
player is maintained in Tau V; this repository's release candidate makes no
new Pocket hardware claim. Full native Linux and Windows gates, long-film, comprehensive
allocation-failure and worker-budget validation remain open.

Tau Encoder 0.9.0 is a macOS host-tested preview. Its native Rust+C path asks
about source errors during video or audio processing and can stop directly owned
tools; custom descendants remain outside its cleanup guarantee. Windows and
Linux runtime, full-film operation, and Pocket playback remain unverified.

The CLI offers a preliminary schema-1 JSON-lines event stream for either C
workflow; generated-media tests checked TNV3/MP3 outputs and hashes on macOS.
Its error mapping is not yet complete. Opt-in native stdin-EOF cancellation
covers trusted directly owned leaf tools on macOS; Python, SIGINT, full
process-tree cancellation and resume remain unsupported. A Windows cross-target
check is build evidence only.

The CLI's `--version --json` reports its package and TNV3 container versions;
external tool versions are explicitly unprobed. The C process adapter exposes
typed errors while retaining its original string API. Its existing Python
process-tree and callback-cleanup limitations are unchanged.

Current development adds `info FILE --json` for header metadata and declared
layout bounds, explicitly leaving index entries and chunks unchecked. The CLI
also discovers tools beside its executable, while explicit tool and checkout
choices take priority. A draft `probe` was withheld after lifecycle review;
resume, full checking and stable error classification remain incomplete.

The process-free reader also offers a partial cross-frame check for anchor
order, timing and indexed audio/reset metadata. It requires stable input and
does not establish full format conformance. Audio interleaving and rolling
rate bounds are checked; subtitle structure is checked separately. Complete
codec syntax remains open. Local reference vectors
were compared against the C checker on macOS.

The CLI exposes `check` scopes for chunk structure, cross-frame rules and
subtitle structure; they explicitly decline to claim full conformance.

Current contract work adds bounded chunk-structure scans, typed dependency
errors, and a plan event describing requested workflow settings and native job
caps. The chunk scan is not a full container check; the plan is not a measured
output report. Probe process containment and deterministic resume remain open.

An experimental `probe` command reports source metadata and, when FFmpeg can
decode a frame, the default reference coded geometry and minimum fitting
level on macOS. Size and time estimates remain unavailable. Its bounded process
supervisor has direct/process-group limitations; Linux runtime and Windows
support remain unverified or unavailable.

On macOS/Linux, individual CLI outputs now use atomic no-replace rename or
hard link, failing with retained staging if neither is supported. The pair
remains non-atomic. An explicit `--allow-nonatomic-copy` restores an
exclusive-copy option for filesystems without atomic methods; interruption
can leave partial files. TauV exFAT rejected hard links and no-replace
rename. A generated TNV3/MP3 test on TauV confirmed the opt-in and host
reference checks; Pocket playback and Windows opt-in remain unverified.

A combined container-preview scope reports valid, invalid or indeterminate
for the implemented cross-frame and subtitle checks on a stable regular file.
It does not decode TVC/TAC payloads or claim full format conformance. Local
30-vector C-reference comparisons and the independent Python vector suite
passed on macOS; native Linux/Windows runtime gates remain open.

The CLI can query SHA-256 identities of explicitly selected tools; hashes
describe bytes read at query time, not semantic versions or later execution.
A complete version handshake remains open.

The CLI direct-child cleanup fallback now stops retrying permanent wait errors
and avoids a blocking caller wait when a reaper thread cannot start. Cleanup
uncertainty still retains staging and can leave a child running; eventual
reaping and process-tree containment are not guaranteed.

Rust+C worker termination and exceptional reaping are now bounded as well.
If cleanup cannot be verified, the workspace is retained and a child can
still be running; this does not establish process-tree cancellation.

Next: continue the process-free reader/checker and the separate C-backed CLI
for Omega's revised consumer contract, including a versioned machine interface,
safe cancellation and deterministic resume. These remain incomplete; see
`docs/CROSS_PLATFORM_CLI_MIGRATION_PLAN.md` and `docs/RUST_ROADMAP.md`.

## Earlier milestone records

Historical codec research, format studies and measurement data accompany the
four specification snapshots; see `docs/RESEARCH_INDEX.md` for their scope and
reproduction constraints. These additions make no new hardware-validation claim.

The research index separates MPEG-4 player history, AV1 subset work, the custom
format family and comparative research; this is a documentation change only.

The README summarizes measured encoder and comparative codec research, with
links to the underlying methods and evidence limits.


Rust work has started: `tau-video/` reads TNV3 headers and individual index
entries and provides a preliminary info CLI. This is host-tested on macOS; it
is not a full checker, decoder or encoder. The cross-platform CI gate and the
remaining requirements are pending. See `docs/RUST_ROADMAP.md`.

Temporary Tau ν audio support is provided by the optional `tau-mp3` CLI using
FFmpeg: a generated 48 kHz host audio round trip passed on macOS. TAC and
subtitles are future work; this does not add MP3 to the TNV3 format.

The Rust crate now has TVC draft 5 range coding and picture/slice-header
parsing. Host tests match a C-generated range-coder fixture and parse local
reference-vector picture headers. Full picture encode/decode is still pending;
no new Pocket or cross-platform codec claim is made.

TVC integer inverse transforms and intra prediction now match synthetic C
reference fixtures across all quantizers and edge availability cases. This is
host-only component evidence; full frame encoding and decoding remain pending.

TVC motion compensation now has host-tested half-pixel, quarter-pixel luma,
and eighth-pixel chroma paths matching generated C-reference cases. Full frame
codec operation and cross-platform performance remain unverified.

The Rust TVC deblocker now filters full YUV pictures; 104 synthetic pictures
matched the C reference on the host. Full frame encode/decode remains pending;
no new Pocket, Quartus or cross-platform performance claim is made.

The Rust TVC coefficient coder and decoder now match a synthetic C reference
across 300 mixed blocks; this is host-tested component evidence, not a complete
frame codec or new hardware result.

TVC macroblock motion-vector prediction and signed Exp-Golomb coding now match
synthetic C-reference cases on the host. The full macroblock parser and frame
encoder/decoder remain unfinished; no new hardware validation is claimed.

The Rust crate now parses intra-picture macroblocks in a TVC slice, including
adaptive quantizers, prediction modes and 4x4/8x8 coefficients. A generated
C-reference slice passes on the host. P/B syntax, full-frame reconstruction,
encoding and hardware validation remain open.

Rust can reconstruct standalone TVC intra pictures to YUV420; first frames of
21 local vectors match the C decoder byte-for-byte on the host. This does not
include P/B frames, full stream playback or encoding, and makes no new Pocket
hardware claim.

Rust `PictureDecoder` now reconstructs I/P/B TVC pictures in display order.
All 199 frames in the 21 local vectors matched the C decoder on the host; a
synthetic I/P/B sequence is checked in public tests. Full stream/container
validation, seeking, encoding, fuzzing and cross-platform performance remain
open. No new Pocket claim is made.

A Rust TVC elementary-stream reader and `decode-tvc` CLI now output Y4M in
display order; the synthetic eight-frame output matches the C decoder on the
host. Full conformance and level validation, TNV3 decoding, seeking, encoder
and malformed-input fuzzing remain open. No new hardware claim is made.

A baseline deterministic Rust intra-picture encoder now emits TVC pictures.
Generated pictures at three sizes and four QPs decoded identically in the Rust
and C decoders on the host. It is intra-only and does not yet enforce stream
levels or rate windows; full encoding, performance and hardware gates remain open.

The Rust baseline now encodes Y4M as intra-only TVC1 with level selection and
size/rate-window retries. Generated streams passed the C decoder and matched
the Rust decoder on the host. P/B encoding, quality/performance gates, TNV3
muxing and resumable operation remain open; no new hardware claim is made.

Rust now muxes TVC1 into video-only TNV3 with CRCs and display timestamps.
The C container checker accepted a synthetic I/P/B stream and three local
level 1–3 vectors on the host. MP3 remains a sidecar; TAC, full container
checking and production encode/decode gates remain open.

Rust also decodes the video track of TNV3 files to Y4M, ignoring audio and
subtitles. Output matched the C demuxer and decoder on all nine local TNV3
vectors on the host. Full checking, audio, seeking and fuzzing remain open.

The Rust stream encoder now uses a zero-motion P picture when mean squared
error against its reconstructed reference is at most 4 across YUV samples; it
starts a new I picture at least every 96 frames. On a generated static 240-frame
host case, file size fell from 354,740 to 19,622 bytes with identical decoded
pixels; this is not a general compression or speed claim. General motion/B
encoding, quality and performance gates remain open.

Rust now also tests small whole-frame integer translations for zero-residual P
pictures, gated by the same YUV error bound; it falls back to I pictures. A
synthetic successive-translation case matches the C decoder on the host.
General block motion, B pictures and quality/performance gates remain open.

Rust now tests zero-motion residual P candidates against intra size and YUV
error; generated successive P pictures matched the C decoder on the host.
This heuristic is experimental and may cost extra encoding work. Motion
residuals, B pictures and comprehensive quality/performance gates remain open.

Rust now evaluates whole-frame motion with residual P pictures against intra
size and YUV error. A generated translated-and-changed frame matched the C
decoder on the host. The search remains coarse and picture-wide; block motion,
B encoding, comprehensive quality and performance gates remain open.

An opt-in Rust block-motion experiment is available; on one generated
12-frame host case it reduced size while improving measured YUV PSNR. The
short sample does not establish general quality or speed; default remains off.
See `docs/RUST_ENCODER_EXPERIMENTS.md` for the data and next gates.

A 12-case short real-clip study did not justify enabling Rust block motion by
default. A sampled precheck retained identical outputs on those cases; its
timings are preliminary. See `docs/RUST_ENCODER_EXPERIMENTS.md` and the linked
measurement data. Longer interleaved quality and speed tests remain open.

Rust has an opt-in library experiment for a three-frame group with two intra
anchors and a zero-motion B picture. A synthetic case passed the C decoder and
TNV3 checker on the host. The stream encoder does not emit B pictures; wider
quality, rate, speed and end-to-end integration remain open.

An opt-in Rust three-frame B stream mode now includes zero-motion residuals
and rate-aware intra fallback. C decode and container checks passed on the
host. In four short clip comparisons it was larger than the normal P encoder
on three clips, so it remains experimental. See RUST_ENCODER_EXPERIMENTS.md.

The opt-in B stream now reuses its preceding intra anchor. On four short host
clips this reduced size versus the prior B prototype on two clips and increased
it on two; it remained larger than the default P encoder on three. Normal P
anchors are not yet shared. See `docs/RUST_ENCODER_EXPERIMENTS.md`.

The opt-in Rust B stream can now share P anchors, with a maximum 96-frame
intra refresh. Four short host clips at three QPs showed mixed size/quality
results versus the normal P path; C/Rust pixels and C container checks passed.
It remains experimental. See `docs/RUST_ENCODER_EXPERIMENTS.md`.

An opt-in B-group guard now compares each proposed group with its I/P
alternative. On 12 short host cases it removed the earlier anime size/PSNR
regression but added considerable encoding time; the default remains unchanged.
C container checks and Rust/C decoded-pixel comparisons passed. See
`docs/RUST_ENCODER_EXPERIMENTS.md`; longer quality and speed gates remain open.

The opt-in B encoder now skips an exact redundant static-pair comparison. On
12 short host cases outputs were unchanged; a generated 2,400-frame static
case improved median time in 12 alternating runs. This does not establish a
general speed result. See `docs/RUST_ENCODER_EXPERIMENTS.md`.

The opt-in B encoder reuses a future anchor when a preceding skip leaves its
reference unchanged and rate limits cannot affect candidate choice. Output was
identical on 12 short host cases; a generated repeat/change clip improved in
a paired timing study. This is not a general speed claim. See
`docs/RUST_ENCODER_EXPERIMENTS.md`.

A 48-frame host film study exposed Rust rate-window starvation at some QPs.
An opt-in conservative per-picture budget completed those cases but changed
quality and often retried QPs; it is not a general fix. See
`docs/issues/001-rust-rate-window-starvation.md` and the experiment record.
Default behavior remains unchanged; look-ahead rate control remains open.

A host-only 1.5× per-picture budget recovered quality in one case but still
failed the same difficult film cases, so it was rejected. See the rate-window
issue and `docs/RUST_ENCODER_EXPERIMENTS.md`; bounded allocation remains open.

A host-only predicted-picture rescue at QP 51 did not resolve the film
rate-window failures. It was not shipped; see the experiment record.

A Tauri/Svelte desktop preview in `tau-encoder/` calls the Rust crate directly
to encode Y4M into video-only TNV3. **Host:** a macOS application bundle was
opened and used to encode a generated frame; the C TNV3 checker accepted the
result. The app enables conservative rate control; audio, progress and cancel
remain separate or unavailable. Windows/Linux CI is configured but has not
been verified; this is not yet a cross-platform release.

The desktop preview now converts ordinary video files to matching `.tnv3` and
48 kHz stereo `.mp3` files, supports a chosen output folder and remembers it.
**Host:** a generated MP4 was processed with the bundled macOS FFmpeg; the C
TNV3 checker and FFprobe accepted the pair. macOS helper bundling includes
source and license materials. Windows/Linux source builds passed the earlier
CI run, but bundled helpers and audio/video hardware validation remain open;
see `docs/issues/002-desktop-pair-platform-gates.md`.

The desktop preview now reports estimated video progress and remaining time
from actual encoded frames and recent speed; audio and saving are separate
indeterminate stages. **Host:** an isolated macOS app run showed changing
progress and ETA on a generated input. Duration estimates may differ from the
actual video, and no Pocket or new cross-platform packaging claim is made.

A Pocket test of a Rust-app encode reported choppy playback and pictures
continuing while paused. The card file passes the host checker and has regular
24 fps timestamps, but is mostly intra-coded; player pause scheduling and
hardware throughput remain under investigation. See
`docs/issues/003-rust-encoder-player-throughput.md`.

A C-encoded comparison played more smoothly on Pocket but later froze while
audio continued. Full host software decoding and an instantaneous-decoder
player simulation completed; real hardware timing and the freeze cause remain
unresolved. See `docs/issues/003-rust-encoder-player-throughput.md`.

The desktop preview now offers the existing segmented Python/C film encoder
and the Rust encoder, with editable `_C` / `_R` suffixes, plus a simple local
TNV3/MP3 viewer. **Host:** both pair-generation paths and preview frame output
passed automated tests; the packaged UI and audio playback still need an
interactive Mac check because the machine was locked. Python 3 remains an
external dependency for C mode; see `docs/issues/004-desktop-preview-validation.md`.

Generated HEVC/H.264 ingest tests exposed a bundled-FFmpeg timestamp case
that repeats output frames, plus source errors that can be hidden despite a
successful encode. Earlier C/Rust Pocket comparison used different FFmpeg
versions and does not isolate the TVC encoder. Candidate changes and the
remaining A/V-sync and real-source gates are in
`docs/issues/005-hevc-input-ingest.md`. No production fix is claimed.

The desktop app now reports bounded source-decoder diagnostics after either
encoder, including when FFmpeg exits successfully. **Host:** generated
video and audio damage exercised the warning path; undetected corruption and
interactive UI checks remain open. Additional synthetic timing tests found
trade-offs in the frame-rate candidate, so its default remains unchanged;
see `docs/issues/005-hevc-input-ingest.md`.

Further host timing tests rejected another end-of-stream candidate: it
added a frame in all 11 generated reference comparisons. A separate MP3
gapless-delay model suggests a possible audio-clock offset, but the actual
Helix decoder and Pocket timing remain unmeasured. See
`docs/issues/005-hevc-input-ingest.md` and `docs/issues/006-mp3-gapless-timing.md`.

The desktop encoder exposes opt-in frame-timing and MP3-delay experiments
for both C and Rust, with existing defaults retained. **Host:** generated
marker pairs tested each setting through both backends; Pocket playback and
actual Helix delay remain unverified. See issues 005 and 006.

Desktop dependency settings now accept remembered explicit paths for FFmpeg,
FFprobe, Python, the C script and its two helper executables. **Host:** a
macOS C encode succeeded with tools in separate locations; native Windows
and Linux packaging and runtime checks remain open. See
`docs/issues/004-desktop-preview-validation.md`.

The Rust encoder now evaluates residual P pictures for unaligned display sizes
using reconstructed padded references. **Host:** synthetic odd and 400x226
chains match the C decoder; a short generated comparison reduced Rust output
size but still showed a substantial C gap. Hardware playback and true encoder
parity remain open; see `docs/RUST_C_ENCODER_PARITY.md` and issue 003.

The default crate now offers a safe process adapter around the existing C
workflow; the normal desktop build uses it and parks the pure Rust encoder
behind an experimental feature. **Host:** generated macOS encode, failure,
warning and feature tests pass. Windows/Linux runtime, packaged UI and Pocket
playback remain open. See `docs/RUST_C_ENCODER_PARITY.md`.

The `tau-video` default build is dependency-free and no longer enables the
process-based C adapter. Applications that use it must explicitly enable
`c-encoder`; Tau Encoder already does. **Host:** core feature combinations
and the desktop C path passed generated-media tests.

The C adapter creates private Unix work directories. **Host:** permission
and adapter tests passed; Windows ACL validation is open (issue 007). A
video-only CLI prototype remains staged after review identified output
publication races and crash limitations; see issue 008.

An early `tau-codecs-cli` source prototype encodes video-only TNV3 through
the C workflow. **Host:** generated macOS smoke and publication checks pass;
Windows, MP3 pairs, FAT/exFAT and Pocket remain unverified or unsupported.
See `tau-video-cli/README.md` and issue 008.

The experimental CLI also offers an opt-in Pocket TNV3/MP3 sidecar profile.
**Host:** generated macOS pair, decode, audio metadata, no-audio cleanup and
no-overwrite tests passed. Pair publication is not atomic; Windows, Linux,
card filesystem and Pocket behavior remain unverified. See issue 008.

A process-group cancellation prototype remains isolated after review found
that descendants may survive and numeric group IDs can race an external
reaper. **Host:** fake-process tests on macOS; no cancellation is claimed
for the app or CLI. See `docs/issues/009-c-encoder-process-cancellation.md`.

MVP focus: the experimental CLI defaults to a TNV3/MP3 pair and discovers
common tools, with explicit overrides. **Host:** macOS generated-pair and
no-overwrite checks passed. Native Windows/Linux app builds previously passed
CI; downloadable executable artifacts and updated CLI builds await CI.
Dependencies remain external; Windows CLI publication is still unsupported.

The MVP CLI now has a Windows direct-copy publication path with exclusive
file creation. **Cross-compile:** Windows code and tests type-check; native
Windows execution is unverified, and interrupted copies can leave partial
new files. This supersedes the earlier Windows-unsupported CLI note.

**CI artifact evidence:** commit `1e5bd30` passed CLI workflow run
`41342981023` and desktop app workflow run `41342981138`; macOS, Windows
and Linux jobs succeeded and uploaded executable-only artifacts. These are
not installers and do not bundle dependencies. CI establishes build and test
success, not Windows runtime or FAT/exFAT publication.

The desktop encoder checks the destination folder and detects existing or
dangling output paths before probing. **Host:** metadata checks and early
error tests passed; this does not prove the folder is writable until the final
files are created. A unique generated `create_new`/sync probe on the TauV
exFAT card succeeded and was removed; no existing media was touched.

An earlier CLI used an exclusive create-and-copy fallback where no-replace
rename was unsupported; that fallback was later removed to avoid partial files. **Host/card:** a generated TNV3/MP3 pair was written and
validated on TauV exFAT, then removed; this did not test Pocket playback.
That earlier fallback could leave partial new files. See issue 008.

**Host/card:** the integrated release CLI encoded a generated one-second
TNV3/MP3 pair directly to TauV exFAT using the no-replace fallback; the C
checker and MP3 probe passed, repeat encoding preserved both files, and the
test files were removed. Pocket playback is not implied.

A streaming Y4M frame-group reader is available in the process-free Rust
core as groundwork for replacing Python segmentation. **Host:** generated
byte parity, bounds and error-state checks passed. It is not yet connected
to the production encoder or a claim that Python is removed.

**Host:** the original HEVC source's first-minute Y4M pixel sequences match
between FFmpeg 6.1.1 and 9.0.2 under matching filters; alternate rounding
changes selections. This does not resolve Pocket playback or full-file timing;
see issues 003 and 005. No source media is published.

An opt-in experimental Rust C-segment orchestrator now handles pre-decoded
Y4M without Python. **Host:** one generated no-retry fixture byte-matched
the Python workflow; a stub exercised boundary retry. Production app/CLI
still use Python. Real C retry, upstream FFmpeg success gating, Windows and
long-film workspace behavior remain open.

The CLI has an opt-in Rust orchestration experiment using FFmpeg and the C
encoder without Python. **Host:** generated 121-frame and 481-frame pair
outputs matched the Python path byte-for-byte; failure tests rejected output
when FFmpeg failed. It remains experimental: workspace growth, real boundary
retry, process-tree cancellation, Windows and Pocket checks remain open.

The opt-in Rust C orchestrator now accepts completed segments in order and
removes their raw and encoded temporary files after rate checking and merge.
**Host:** generated two-segment Python byte parity and a simulated boundary
retry passed. The merged TVC and final TNV3 still use temporary disk, the
budget is projected, and real C retry/full-film gates remain open.

**Host:** a generated 1,441-frame, 400x300 comparison of the Python and
experimental Rust C workflows produced byte-identical TNV3/MP3. Periodic
temporary-workspace sampling measured about 260 MB versus 173 MB, respectively;
one run each, not a performance benchmark. See the CLI migration plan.

The desktop app now separates Encoder, Decoder (local preview only), and
Settings. Its default remains the Python-orchestrated C workflow; users may
select an experimental Python-free Rust orchestrator that uses the same C
codec and a projected temporary workspace budget. The pure Rust codec stays
behind its build feature. **Host:** generated-media tests and frontend checks
cover the workflows; long-film, Pocket, and Windows/Linux runtime validation
remain open. See `docs/issues/004-desktop-preview-validation.md` and
`docs/issues/009-c-encoder-process-cancellation.md`.

The desktop app uses independent semantic versions; the 0.5.0 preview introduced
version checks covering npm, Tauri, Cargo and their lockfiles. This does
not change codec-format or crate versions.

**Host:** Tau Encoder 0.2.0 did not report intermediate progress for the
Python-free C workflow; its video phase could appear stuck at zero while
working. Version 0.2.1 reports accepted segments and an estimated video time.
Pocket playback and full-length validation remain open; see issue 010.

Tau Encoder 0.2.2 explains common source container and HEVC decoder warnings
and retains the original diagnostics. It does not claim to repair corrupt
source data. **Host:** the reported file produced Matroska errors and missing
HEVC reference pictures; output quality and Pocket playback are unverified.

Tau Encoder 0.3.0 shows detected dependency locations and allows selecting
an installation. Its macOS bundle uses FFmpeg 9.0.2; Windows and Linux native
runtime and packaging validation remain open.

Tau Encoder 0.4.0 offers output size and C codec level selection and local
preview scaling. **Host:** short generated level 1–3 encodes passed in both C
workflows; higher-level Pocket playback remains unverified; see issue 011.

Tau Encoder 0.5.0 selects width with automatic height and checks the actual
scaled dimensions against codec levels before encoding. **Host:** generated
width and level tests passed; Pocket playback is unverified.

**CI:** the 0.5.0 Linux app job passed; the Windows job stopped before
compilation due to a line-ending assumption in the version check. The check
now accepts Windows line endings. CI rerun 41466093349 passed on macOS,
Windows and Linux; native Windows/Linux runtime remains untested. See issue 002.

The Tau Encoder 0.5.0 preview release documents the macOS bundled helpers and
separately supplied Windows/Linux dependencies. Windows and Linux runtime and
Pocket playback remain unverified. See issue 002.

A 0.6.0 development preview adds a stoppable source preflight, warning choice
and saved continue preference. **Host:** controlled process checks, the user’s
previously reported source, and a synthetic UI preview passed. The preflight
can require a full decode; live prompts during encoding and safe cancellation
of encoder worker trees remain open. See issues 009 and 012.

A 0.7.0 development preview adds decisions for unacknowledged source errors
after video or audio processing and before final output publication. **Host:**
generated tests covered stop and continue for both C workflows. Prompts are
still deferred until a phase finishes; immediate encoder cancellation remains
open. See issues 009 and 012.

A 0.8.0 development preview accepts a cooperative stop during video or audio
processing. The current phase and its workers finish, then staged files are
discarded before publication; requests are refused once saving starts. **Host:**
generated stop and publication-race tests passed. Immediate worker termination,
true live prompts and Pocket/native Windows/Linux validation remain open.
See issues 009 and 012.

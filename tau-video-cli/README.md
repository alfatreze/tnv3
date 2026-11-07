# Tau video CLI prototype

This standalone crate sits beside `tau-video` and does not modify the Tau codecs checkout.

On macOS, the default command writes a TNV3 video and a matching MP3 sidecar:

```sh
cargo run --release -- encode input.mp4 output.tnv3
```

The checkout is discovered from the current directory or `TAU_CODECS_ROOT`. Python 3 is discovered on `PATH`. FFmpeg and FFprobe search explicit arguments first, then the CLI executable directory, then `PATH`. C helpers search explicit tool arguments first; an explicit `--codec-root` selects its helper paths without fallback, otherwise the executable directory precedes the discovered checkout. Tools are never downloaded. To supply tools directly, use `--codec-root PATH`, `--script PATH`, `--python PATH|NAME`, `--ffmpeg PATH|NAME`, `--ffprobe PATH|NAME`, `--tvc-encoder PATH|NAME`, and `--tnv3-tool PATH|NAME`. Bare executable names are resolved on `PATH`; Unix lookup skips files without execute permission. Paths work for bundled tools outside the checkout. The script and C helpers can be omitted when the checkout supplies them. Use `--video-only` to omit MP3. `--qp` defaults to 30 and `--jobs` to 1.

The MP3 uses the current Tau encoder settings: padded audio limited to the encoded frame duration, 48 kHz stereo, 128 kbit/s, and stripped metadata. No independent MP3 analysis or transformation is performed.

`--profile pocket-sidecar@1` selects the current TNV3 plus MP3 pairing. The
legacy name `pocket-sidecar` remains an alias; unsupported profile versions are
rejected. This selection does not establish Pocket playback compatibility or
provide the planned Host preview and Custom profiles.

## Container metadata

`tau-codecs-cli info FILE --json` returns schema-1 header metadata and the
reader's validation scope. It checks the header CRC and declared layout bounds;
it does **not** read index entries, inspect chunks, or certify the container.
Without `--json`, it prints a human-readable summary. Findings return exit 1,
usage errors 2, and I/O errors 3. JSON errors include a broad `kind` (`format`,
`usage`, or `io`). This preliminary command is not the full
container `check` required by Omega.

`tau-codecs-cli check FILE --scope chunk-structure --json` explicitly scans
indexed chunks and reports typed findings. `scan_complete` describes that
limited scan; `full_conformance_checked` is always false. Invalid headers are
reported as findings in the requested scope. A successful scoped
scan is not a full container validation. `--scope cross-frame` adds the partial anchor, timing and audio/index checks;
it stops at the first finding. `--scope subtitle-structure` validates subtitle layout and contents separately,
subject to a 64 MiB implementation limit. No scope verifies complete conformance.
`--scope container-preview` combines cross-frame and subtitle checks, returning
VALID, INVALID or INDETERMINATE. It requires a stable regular file, rejects final
symlinks, and does not decode TVC/TAC payloads or claim full conformance. The
scope flag is required until a complete checker exists.

`check FILE --quick --json` checks the header and declared layout, then the
first, middle and last unique picture chunks. Its report lists selected and
attempted targets and explicitly marks the scan incomplete. TVC/TAC payloads
are not decoded; a successful sample is not full validation. The input must
remain stable during the scan.

## Source metadata probe preview

`tau-codecs-cli probe INPUT --json [--ffprobe PATH] [--ffmpeg PATH]` runs FFprobe without a
shell and reports selected source metadata. It does not validate the full
stream. When FFmpeg is available, it decodes
one frame with the default 400x360/24 fps reference filter and reports the
coded output dimensions and minimum geometry-fitting TVC level. Custom
scripts or settings may differ; source SAR is reported separately and is not
necessarily baked into square-pixel dimensions. Output size and encode-time
estimates remain unavailable unless `--sample-estimate` is requested. The optional
sample encodes up to the first 48 frames with the default filter and QP 30 and
reports video-payload size and sequential video-stage time projections. These
are not bounds and can differ substantially with later scene complexity,
segment behavior or parallelism; audio, container and mux work are excluded.
Whole-output estimates remain unavailable. Sampling requires FFmpeg and a TVC
encoder; its output-size check is polled and can briefly overshoot its cap.
FFprobe selection follows explicit path, adjacent executable, then PATH. FFprobe
may interpret input protocols and media references; this command does not impose
a network policy. A tool-reported missing or unreadable input currently returns
code 1 because FFprobe does not expose a stable structured cause.

Execution is supported experimentally on macOS and Linux; only macOS runtime
has been tested. Windows and other targets report unsupported when invoked
with an available FFprobe. This standalone supervisor assumes exclusive child
wait ownership. It bounds captured stdout to 1 MiB and stderr to 16 KiB and
uses a 30-second timeout. Cleanup errors are reported, and arbitrary descendants
that escape the process group or close their pipes are not guaranteed to stop.
Signal and cleanup failures may leave a child running.

## Scoped capability handshake

`tau-codecs-cli handshake --json` reports the CLI version and scoped format
capabilities. Optional explicit `--ffmpeg`, `--ffprobe`, `--tvc-encoder`, and
`--tnv3-tool` paths are hashed using the tool-identity checks. No tool is run
or discovered on PATH by default. Add `--probe-versions` to execute selected
tool version switches on macOS/Linux under bounded supervision; returned lines
are unvalidated responses, not verified semantic versions. Hashing and querying
are separate operations and do not bind later execution or guarantee tool
compatibility. Escaped descendants remain outside cleanup guarantees.

## Explicit tool identity

`tool-identity --json --ffmpeg PATH` (and `--ffprobe`, `--tvc-encoder`,
`--tnv3-tool`) hashes explicitly supplied regular files; no PATH search or tool
execution occurs. It reports SHA-256 and `version: null` because the C tools
do not expose authoritative versions. It rejects final symlinks, nonregular
files and files over 2 GiB, and detects descriptor metadata changes during
reading. Parent path components may still be symlinks. The result describes
bytes read at query time; it is not an immutable snapshot and does not bind a
later execution. macOS host-tested; Linux runtime unverified, Windows
unsupported. `--version --json` remains unchanged.

## Machine events

See [CLI machine events](../docs/CLI_MACHINE_EVENTS.md) for field meanings and
consumer limitations.

`--version --json` prints one JSON object and exits. Schema 1 reports the CLI
package version and the TNV3 container version understood by the linked Rust
crate. `tools_status` is `not_probed`; `tools.tvc_enc`, `tools.tnv3`,
`tools.ffmpeg`, and `tools.ffprobe` are `null`. This deliberately does not
claim versions for external programs that may be selected later through PATH
or command-line options. TVC/TAC/TST draft versions are not reported because
the CLI currently has no authoritative version constants for them. This is a
useful partial version endpoint, not the complete Omega version handshake.

`--json-events` writes schema-1 JSON Lines on stdout; diagnostics remain on stderr. The `plan` event includes `effective_settings`, describing requested workflow parameters, including the native worker cap and projected workspace budget. Geometry is a maximum aspect-fit target, and actual dimensions are decided from the source. QP is requested and may be raised for level limits. External scripts and tools can ignore their arguments, so this is not a measured output report. The event sequence is `plan`, zero or more `progress`/`warning` events, then `done` or `error`. `plan` reports the selected profile, requested QP/jobs and backend. `done` reports output paths, sizes, SHA-256 digests, level, numeric `reencoded`, and `reencoded_kind` (`pictures_raised_for_level` or `joint_segments`). Non-UTF-8 paths are rendered with Rust's lossy display conversion, so distinct invalid byte sequences can render identically. Implemented exit codes are 0 success, 1 unclassified workflow/encode error, 2 usage, and 3 I/O or dependency failure. Automatic tool or checkout discovery failures now use dependency code 3 rather than usage code 2; malformed options still use code 2. JSON error events add a `kind` field (`usage`, `invalid_input`, `dependency`, `io`, `cancelled`, or `unclassified`). Code 4 indicates verified native cancellation.

For the C-segment path, progress reports accepted input frames and segment number; total frames are `null` because the streaming Y4M source does not declare a count. Output byte count is `null` until publication. The legacy Python path reports completed segments because its adapter does not expose frame totals. These values describe completed work, not a time estimate. A broken event stream is remembered while active encoder work drains and reaps; staged output is discarded afterward. If the final event cannot be written after publication, the CLI reports the published paths on stderr. The CLI uses typed spawn and I/O errors from `tau-video::c_encoder`; the experimental orchestrator still returns string errors. A failed external workflow can hide an underlying tool or I/O failure and remains unclassified as code 1. This mapping is preliminary and does not meet Omega's stable error-code contract. Preflight, staging, publication and reporting I/O use code 3. Python-workflow cancellation, full process-tree teardown, and resume remain unimplemented; this does not satisfy the complete Omega contract.

On Unix, `--rust-c-orchestrator` also handles SIGINT cooperatively. With
`--rust-c-orchestrator --control-stdin`, closing the control stdin pipe
requests cancellation. A stdin read failure also requests cancellation; ordinary
stdin is ignored unless this flag is supplied. If cancellation wins before
publication, the CLI attempts to stop its directly owned FFmpeg and C tools.
A verified cancellation returns code 4 without publishing the pair; cleanup or
diagnostic failures can instead return an error. Once publication wins, cancellation is too
late and saving continues. If cleanup cannot be verified, an error is returned
and staged files may be retained for recovery. This experimental path assumes
trusted leaf executables and no external child reaper: arbitrary descendants,
supervisor crashes and custom wrappers are not contained. If child teardown
cannot be verified, the CLI may exit while a child still runs; a background
waiter cannot outlive the CLI process. It has been exercised
on macOS only; Windows and Linux runtime behaviour remains untested.

`json_events_smoke.py` optionally exercises both backends with generated media
and verifies JSON output, the reference TNV3 check, MP3 decoding, sizes and
SHA-256 values. Build the debug CLI first, then set `TAU_TEST_FFMPEG`,
`TAU_TEST_FFPROBE`, `TAU_TEST_TVC`, `TAU_TEST_TNV3`, and `TAU_TEST_C_SCRIPT` to
the matching local tools and script. Set `TAU_TEST_CLI` to override the debug
executable, or `TAU_TEST_PYTHON` to select Python. Temporary files are removed
after the run; this is host evidence, not Pocket or full conformance testing.

`--rust-c-orchestrator` opts into a macOS-host-tested experimental path. FFmpeg decodes video to Y4M stdout using the desktop 400x360 aspect-fit, 24 fps profile; Rust streams that pipe into the C segment encoder with the Python workflow's default 480-frame segment size and infers the smallest fitting level from the actual Y4M header. A 128 MiB per-segment Y4M cap fits 480 full-size desktop frames including headers and metadata. At most `--jobs` segment inputs are pending; after a segment passes its rate-window check (and any `-P` retry), its TVC chunks are appended to the merged seekable TVC and its Y4M/TVC files are deleted. The merged TVC and checked TNV3 remain in the private workspace through mux. `--workspace-mib N` sets the projected workspace cap (512 MiB by default); it is checked as the merged stream and mux output grow, but is not a hard filesystem quota. FFmpeg stderr capture is capped at 4 KiB. On Y4M errors the CLI attempts to stop and reap the direct FFmpeg child; if cleanup cannot be verified it retains staging and a child may remain running. It does not manage a process group, so descendants may outlive the child. If a descendant keeps the inherited stderr pipe open after FFmpeg exits, the CLI waits at most two seconds, returns an error without publication, and detaches the diagnostics reader until that pipe closes. The TNV3 remains staged until FFmpeg exits successfully; the MP3 sidecar is then encoded and the pair is published. A nonzero decoder exit or missing audio leaves neither final name published. Generated 121- and 481-frame fixtures matched the Python C workflow byte-for-byte for TNV3 and MP3; the 481-frame case used two segments on each path. Windows runtime behavior is unverified.

Run the reproducible 481-frame two-segment parity smoke with explicit tools:

```sh
python3 tau-video-cli/rust_c_parity_smoke.py \
  --codec-root "$TAU_CODECS_ROOT" \
  --ffmpeg "$TAU_FFMPEG" \
  --ffprobe "$TAU_FFPROBE"
```

It generates media in a temporary directory, checks both TNV3 files, compares TNV3 and MP3 bytes, reports fallback warnings, and removes the generated source and outputs when it exits.

Both outputs refuse to replace existing files. On Linux and macOS, the CLI
stages data in a private sibling directory, syncs each staged file, and uses
a no-replace rename or, when unsupported, an atomic no-replace hard link.
By default, publication fails and retains staging if neither is supported.
The tested TauV exFAT volume rejected both atomic mechanisms. To permit direct
output on such filesystems, pass `--allow-nonatomic-copy`: this explicitly
permits exclusive-create copying, so interruption or a write/sync failure can
leave a partial final file. A warning is reported and failed staging is retained
for inspection. A generated TNV3/MP3 pair was successfully published with
this opt-in on TauV exFAT and passed the C reference and MP3 host checks; Pocket playback
was not tested.
The TNV3 and MP3 are still published one at a time; if the second fails the
first can remain. The output directory must be trusted and not group/world-
writable. The staged TNV3 retains its private file mode.

On Windows, the CLI currently refuses to encode unless the same explicit
`--allow-nonatomic-copy` option is supplied. Its exclusive-create copy can
expose partial files, staging uses the system temporary directory, and private
ACL/reparse-point protections and Windows runtime behavior are unverified.

The generated `smoke.py` and `pocket_sidecar_smoke.py` helpers remain optional. The latter verifies generated TNV3 plus MP3 output, format properties, and no-overwrite behavior on the host; it does not test Pocket hardware.

## CI artifacts

At commit `1e5bd30`, CLI workflow run `41342981023` and desktop workflow run
`41342981138` succeeded on macOS, Windows, and Linux and uploaded executable-only
artifacts. They are not installers and do not bundle dependencies. These CI
results confirm build/test jobs only; Windows runtime remains unverified. A
separate macOS-hosted exFAT card run is documented in issue 008; it does not
establish Windows FAT/exFAT behavior.

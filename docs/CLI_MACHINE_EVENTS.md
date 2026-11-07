# CLI machine events, schema 1 preview

`tau-codecs-cli encode ... --json-events` emits one UTF-8 JSON object per
line on stdout. Human diagnostics remain on stderr. Each object has `schema: 1`
and an `event` discriminator. This is a development interface: whole-tree
cancellation, resume, complete typed error classification and all Omega
requirements are not yet implemented.

For example, the first event of a native pair job can be:

```json
{"schema":1,"event":"plan","profile":{"id":"pocket-sidecar","version":1},"input":"input.mp4","files":2,"qp":30,"jobs":1,"backend":"experimental-rust-c-orchestrator","resume":false,"cancellation":"sigint-direct-leaf"}
```

- `plan` gives a profile object with `id` and `version`, input, output count,
  QP, job count, backend and explicit cancellation/resume status. Its
  `effective_settings` object describes requested workflow parameters, not
  measured output; the native job limit is capped at six and the requested
  QP may be raised by level retries. External tools may ignore parameters. It follows
  preflight; a preflight failure can therefore produce only an `error`.
- `progress` gives elapsed seconds and the backend's available counters. The
  native path reports accepted `frames` and `segment`; the Python path reports
  completed `segment` and `segments`. Unknown frame totals and output byte
  counts are `null`, not zero. These are not time-to-completion estimates.
- `warning` provides a human-readable `message`; it does not certify that
  source data was repaired or that the output is defect-free.
- `done` gives `files`, `sizes`, `sha256`, `level`, `reencoded`,
  `reencoded_kind` and elapsed seconds. Arrays correspond by position. The
  kind distinguishes individual pictures raised for level limits from joint
  segments retried by the native orchestrator.
- `error` contains a numeric `code` and human-readable `message`. Code 2 is
  usage, 3 is a recognized I/O failure, 4 is a verified native cancellation,
  and 1 is an unclassified workflow or encode failure. An additive `kind`
  distinguishes usage, dependency, io, cancelled and unclassified errors.
  Automatic missing dependencies now use 3 instead of 2. Underlying string-only
  adapters make error classification incomplete.

Normal successful jobs emit one plan and one final done; other events may
occur between them. Errors or a broken output pipe can prevent either record
from arriving. If stdout closes during active encoding, the CLI waits for the
current encoder to finish and be reaped before discarding staged output; that
failure alone does not interrupt work. `--control-stdin` separately opts the
native C workflow into cancellation on control-pipe EOF; only directly owned
trusted leaf tools are covered. Unix native SIGINT is also handled; Python
workflow SIGINT cancellation remains unsupported.
Cleanup or diagnostic failures can take precedence over cancellation and may
retain staging; a child may still run after the CLI exits if teardown could
not be verified. For the native workflow, the plan reports `sigint-direct-leaf` on Unix, or
`sigint-or-stdin-eof-direct-leaf` when control stdin is enabled. Windows
reports stdin EOF support only when enabled; the legacy Python workflow does
not support either cancellation mode.
A final-event or hashing failure may
occur after publication, in which case stderr identifies the published paths.
Two output names cannot be atomically committed as one pair.

Paths in JSON are display strings. On Unix, paths with invalid UTF-8 are
rendered lossily and cannot be round-tripped from the event alone. Consumers
must retain their original path values rather than use event strings as an
authority for filesystem operations.

`--version --json` is a separate one-object response, not an encode event. It
reports the CLI package version and TNV3 container version. Tool versions are
`null` with `tools_status: "not_probed"`: the command does not discover or run
external tools and therefore cannot establish a compatible encoder toolchain.

On macOS, a generated-media smoke test exercised both C workflows and checked
the emitted JSON, SHA-256 values, TNV3 reference-checker result and MP3 decode.
This is host evidence, not independent full-container validation, Windows or
Linux runtime qualification, or Pocket playback evidence. See
`tau-video-cli/README.md` and `tau-video-cli/json_events_smoke.py`.

`info FILE --json` is a separate one-object response with `command: "info"`,
header metadata and an explicit validation scope. It validates the header CRC
and declared layout bounds, but does not read index entries or chunk payloads;
it must not be used as a complete container check. Its error object contains
`code`, `kind` and `message`; malformed headers return kind `format` / code 1,
usage kind `usage` / code 2 and I/O failures kind `io` / code 3.

`check FILE --scope chunk-structure --json` reports the bounded reader scan,
its findings, omitted count and whether that scan completed.
`full_conformance_checked` is false: timestamps, references, subtitles and
complete picture/audio syntax are outside this scope. A scoped success does
not establish container validity. The scope flag is required until the complete
checker is implemented.

`check FILE --scope cross-frame --json` runs the partial cross-frame check on
stable input. It stops on the first finding, so `scan_complete` is false after
a format finding and counts are unavailable. It does not validate complete
TNV3 conformance; see `docs/TNV3_CROSS_FRAME_CHECK.md`.

`check FILE --scope subtitle-structure --json` validates only the subtitle
area. It returns zero tracks when subtitles are absent and an implementation
limit for areas larger than 64 MiB, as a structured `limit` rather than an
invalid finding. Like the other scopes, it never claims
full container conformance.

`probe INPUT --json` returns one source-metadata object (`scope` is
`source-metadata-and-default-profile-first-frame` and full packet validation
is not performed), or an error object.
The default-reference geometry and minimum geometry-fitting level are reported
when a first-frame FFmpeg probe succeeds; coded dimensions are not necessarily
square-pixel dimensions and custom workflows may differ. With `--sample-estimate`, the optional measured first-48-frame sample reports
video-only payload and sequential stage projections under `estimate`; total
output size and end-to-end time stay null. The projection is not a bound and
excludes audio, container, mux and the effects of parallel segment execution.
This is not the complete Omega probe. FFprobe-reported
input failure is currently an
unclassified tool failure (code 1). The supervisor and platform limitations
are in the CLI README.

`check FILE --scope container-preview --json` runs the implemented cross-frame
and subtitle checks together. `status` is `VALID`, `INVALID`, or
`INDETERMINATE`; exit codes are 0, 1 and 3 respectively. It requires a stable
regular file and a non-symlink final path on macOS/Linux. It does not decode
TVC/TAC payloads, and `full_conformance_checked` remains false.

The encode plan reports publication policy. `atomic-no-replace-required`
fails if atomic rename/link publication is unavailable. Explicit
`--allow-nonatomic-copy` reports `atomic-preferred-nonatomic-copy-opt-in` and
a partial-file risk warning; successful fallback also emits a warning event.
The TNV3/MP3 pair is not atomically committed.

`tool-identity --json` is a separate explicit-path query. It reports file
SHA-256 digests and `version: null`, with no claim that the selected file will
be used unchanged later. It does not execute tools or implement a complete
version handshake. See the CLI README for path, platform and mutation limits.

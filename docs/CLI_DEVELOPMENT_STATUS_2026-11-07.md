# CLI and Rust codec development status — 2026-11-07

This is a development handoff, not a release or a claim that the Omega CLI or crate contract is complete. This snapshot was prepared from the Tau codecs working tree based on commit `36f012a`; consult Git for the current publication state. Preserve unrelated working-tree changes and do not reset or replace the tree wholesale. The player remains a separate project.

## Integrated and host-verified

- The C encoder remains the reference. The CLI supports the Python/C and experimental native Rust+C workflows, schema-1 machine events, TNV3 plus temporary MP3 sidecars, output staging and no-overwrite publication. Two output files are not an atomic pair. On TauV exFAT, a generated-media test confirmed the explicitly opted-in `--allow-nonatomic-copy` path and host C/FFprobe checks; interruption can leave partial final files. This is not Pocket playback evidence.
- `info` reports header metadata and scoped error kinds. Explicit `check` scopes cover chunk structure, cross-frame rules, subtitle structure and a combined container preview; `--quick` samples selected chunks. All are explicitly incomplete and do not establish TVC/TAC payload conformance. An unqualified full `check` was intentionally withheld ([issue 017](issues/017-default-check-partial.md)).
- `probe` reports source metadata and reference geometry. Optional `--sample-estimate` encodes up to 48 initial frames and reports video-only payload and sequential stage projections; whole-output bytes and end-to-end time remain null. Estimates are not bounds and exclude audio, mux, parallelism and scene variation.
- `handshake --json` reports scoped compiled capabilities and explicit-file SHA-256 identities. Optional `--probe-versions` returns bounded, **unvalidated** version-switch responses on macOS/Linux. Hashing, querying and later execution are separate; no toolchain compatibility or immutable binding is established. `tvc_enc` and `tnv3` sources now expose tool CLI version 0.1.0, separate from format versions; existing packaged binaries predate these source changes and require rebuilding.
- `pocket-sidecar@1` names the existing profile, with `pocket-sidecar` retained as an alias. It is not a hardware-compatibility certification. Host preview and Custom profiles remain open.
- Native Rust+C cancellation supports Unix SIGINT and optional stdin EOF for directly supervised trusted tools, with a cancellation/publication decision. Bounded exceptional cleanup reports uncertainty and can retain staging; descendants may survive. Python workflow cancellation, Windows Ctrl-Break and complete process-tree ownership remain open.
- The Rust TVC elementary-stream decoder has an opt-in detailed error API. Its original exhaustive error enum remains; the compatibility entry point now rejects nonzero TVC1 reserved bytes and some error messages differ. A successful decode is not complete payload-conformance evidence; malformed versus unsupported picture syntax can remain ambiguous.

## Verification and artifacts

A fresh current-source snapshot passed the macOS CLI suite (72 unit tests plus integration suites), the feature-enabled Rust crate suite (54 unit tests plus integration suites), the app backend suite (31 tests), Svelte checks and app version checks. Windows cross-target compilation passed for the CLI work, not Windows runtime. Linux runtime and Pocket playback have not been validated for these changes. One intermittent broken-progress-stream test failure is unresolved; later repeated runs passed and the test now preserves diagnostics ([issue 019](issues/019-progress-stream-test-intermittent.md)).

The local, ad hoc signed macOS Apple Silicon app `local-builds/Tau Encoder-0.9.1-test.1-macos-arm64.app` and its ZIP, plus `local-builds/tau-codecs-cli-0.4.0-test.1-macos-arm64`, were built **before** the newer work in this handoff. They do not contain all current changes. Their generated-media CLI smoke passed for both workflows; GUI interaction, Pocket playback and notarization were not verified. No new build was made after the latest changes.

## Open next work

1. Complete the checker: accurate invalid/unsupported/I/O/limit outcomes and offsets, required container rules and finding aggregation, and independently validated TVC/TAC coverage. Decoder success is not a full conformance result; see [issues 017](issues/017-default-check-partial.md) and [020](issues/020-tvc-payload-check-scope.md).
2. Finish cancellation: safely coordinate Python and its FFmpeg/C children, address Windows Ctrl-Break and process ownership, and validate cancellation/publication boundaries. The isolated Python prototype is unfinished and not integrated.
3. Complete tool/version compatibility semantics and bind the selected identity to the tools actually executed; continue stable workflow error classification and supported Host preview/Custom profiles.
4. Make full-pair size and end-to-end time estimates defensible and explicitly identify settings and uncertainty. The isolated full-estimate draft is unfinished and not integrated.
5. Run an integrated current-source test suite, native Windows/Linux runtime tests and Pocket playback when available; investigate the intermittent progress-output failure before relying on the affected guarantee.
6. **Parked by owner:** deterministic resume and checkpoint integration until the final format is complete and frozen. The isolated prototype is not production functionality.
7. C library/FFI extraction remains conditional on a demonstrated need and a separately reviewed design; the current C process workflow remains the reference.

The ignored Tau V `work/cli-full-estimate-new` and `work/python-cancel-impl` directories contain incomplete drafts. Their agents stopped at the session usage limit; do not copy them into the live tree without fresh comparison, completion, tests and independent review. The codec project's [issue records](issues/) preserve the detailed decisions and evidence; local Tau V handoff and audit records contain additional historical context. Some older entries in the migration plan are historical; its dated current-status section supersedes them.

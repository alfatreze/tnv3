# Decision: keep a reusable workflow outside the format core

2026-11-05. Proposed; not yet implemented.

Tau Omega's revised consumer requirements place its reader and decoder in
process and launch the encoder from a host adapter. Other applications may
also want a Rust interface to the same encoding workflow. Keep `tau-video`'s
default core safe, dependency-light and process-free. Move shared source/tool
discovery, staging, progress, diagnostics and publication into a separate
host-side workflow crate when its interface is ready. The CLI and desktop app
can then use that adapter; Omega may still execute the CLI as a sidecar.

Alternatives considered: duplicating the current CLI and app orchestration
would preserve the present code but let settings and error handling drift.
Putting the workflow in the default `tau-video` core would violate its
in-process consumer contract. Extracting a C library now would add ABI and
lifecycle obligations without evidence that the process boundary is the
current bottleneck; keep the optional library/FFI decision separate.

The prospective adapter should accept explicit tool paths and resolved
profiles, expose typed job state and progress, retain source diagnostics,
and keep output staged until validation and publication. Cancellation must
state which processes were stopped and whether cleanup was verified. Two
output names are not atomic as a pair. A versioned CLI JSON-lines protocol
should translate these outcomes without asserting complete container
validation or deterministic resume before those are implemented.

Evidence today is host-only: the desktop Rust+C path supervises directly
owned tools and has a macOS preview, while arbitrary descendants, Windows and
Linux runtime, full-film behaviour and Pocket playback remain unverified.
Next gate: settle the workflow API with its consumers and tests, then extract
one responsibility at a time without changing the Python rollback path.

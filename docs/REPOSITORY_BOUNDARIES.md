# Repository boundaries

## TNV3 (`tnv3`)

Keep host-side TVC/TAC/TNV3 tools, the `tau-video` Rust workspace, a thin CLI,
conformance tests and release packaging together initially. They share format
versions, vectors, compatibility tests and a release cadence. The executable
film wrapper may call FFmpeg; the Rust library must remain portable and not use
external processes for core functions.

## Tau V

Keep the Pocket player, firmware, RTL, hardware tests and device packaging in
Tau V. Tau V currently owns the normative format specifications. This project
ships pinned snapshots; changes must be coordinated and validated on both sides.
Avoid two independently evolving normative specifications. Once the formats and
conformance workflow are stable, transfer authority deliberately or extract a
small shared specification/vector repository only if the extra versioning cost
is justified.

## Other applications

Tau Omega and future end-user applications stay in their own repositories and
consume a versioned `tau-video` crate or released CLI. Avoid embedding an app,
Tauri dependencies or UI in the codec library. A separate GUI utility is useful
only when it has its own team/release cycle; initially it can live as a separate
application package without forcing a new repository.

## Releases and compatibility

Use a workspace to release the Rust crate and CLI with separate versions when
stable; publish host binaries per platform after native CI passes. Pin tested
format draft versions and conformance-vector hashes. Keep historical references
and upstream MIT notices; audit source and fixture rights before redistribution.
The C reference and Rust implementation should cross-check each other without
requiring either consumer to import the Pocket player.

# TNV3 extraction and alpha release (2026-10-31)

## Decision

Create a public `alfatreze/tnv3` repository and a local sibling folder
`Tau Alpha/Tau codecs` for the host-side C TVC encoder/decoder, TAC audio
encoder/reference decoder and fixed-point model, TNV3 container tools, film
wrapper and future `tau-video` Rust workspace/CLI. Tau V retains the Pocket
player, firmware, RTL and hardware validation. The original encoder files remain
in Tau V for existing consumers; this extraction does not remove them.

Keep codec components and the future Rust crate together initially because
they share format drafts, conformance vectors and release compatibility.
Applications such as Tau Omega remain separate consumers of versioned libraries
and tools. A separate formats/vectors repository or standalone GUI may become
appropriate when ownership, release cycles or teams diverge; splitting them now
adds coordination without established benefit. Tau V remains normative authority
for TVC/TAC/TNV3/TST; TNV3 contains explicitly pinned snapshots. Transfer or
split that authority only deliberately and with cross-project conformance gates.

Alternatives considered: retain all host tools in the player repository (harder
independent releases), create separate encoder/audio/container/Rust repositories
now (excessive version coordination), or promote all prototypes as production
(unvalidated platform/failure risks). The release uses validated serial TVC by
default and makes the promising parallel-slice path a separate explicit opt-in.
It does not introduce additional slices. A requested one-worker budget or pool
startup failure selects the serial slice path; the other allocation failures and
native Windows pool backend still need work.

## Published state

- Repository: https://github.com/alfatreze/tnv3, public, main at
  `54a577c6511d9fa3453a6d5184b054ebbba1a956` at release.
- Prerelease: https://github.com/alfatreze/tnv3/releases/tag/v0.1.0-alpha.1.
- Source archive SHA-256:
  `b17eb570d01c7c2960a185e4860c45bd1edaccd9ea878609e856fbc83a0d3017`.
- macOS arm64 binary archive SHA-256:
  `6bb69fadae5b80350d66023cd700c7e392c7ef6214bfaae34328a1984fab590f`.
- `SHA256SUMS` is attached to the prerelease. Uploaded GitHub asset digests match.
- Source extraction provenance is in the new project's SOURCE_MANIFEST.json;
  original Tau/HarpMudd MIT notices are retained. Existing media-derived vectors
  were not redistributed while rights/provenance review remains pending.

## Evidence and remaining gates

Local macOS ARM64: clean source archive extraction/build, 38-file manifest,
synthetic TVC encode/decode, TAC reference/fixed-point encode/decode and transform
check, TNV3 mux/demux/check, slice-vs-serial equality and forced serial fallback
pass. The extracted Apple Silicon binary archive independently passes the same
smoke. Binaries target macOS 12.0 minimum but were run only on macOS 26.6.2;
they are not independently signed/notarized. Source and binary archives are
under work/encoder_lab and attached publicly.

The extracted serial encoder passes the full 4-clip/3-QP gate with no-recon
comparison, mean BD-rate -1.52% vs frozen baseline and parser-cost checks.
The extracted slice build matches serial on the same -S3 corpus; that forced
layout fails the unsliced quality gate as expected. A 24-frame generated
800x480 level3 film + audio produced identical serial and parallel TNV3 output;
one- and two-worker settings pass. This is host-only evidence, not a new
hardware or full-film claim. Final source commit GitHub Actions smoke passed on
Ubuntu and macOS: https://github.com/alfatreze/tnv3/actions/runs/41214859332.

Native Windows runtime, full Linux corpus/performance, older macOS runtime,
all allocation failures, long diverse films and new Pocket/FPGA timing remain
open. The `tau-video` Rust crate is not implemented; the new project's
`docs/RUST_ROADMAP.md` records the consumer's requested container-reader-first
order. The user's requested Rust requirements remain the input for that work;
no claim of a published Rust library is made. No Tau V SD card, hardware VM,
decoder, RTL or firmware changes were made during extraction.

## Alpha.2 research update

https://github.com/alfatreze/tnv3/releases/tag/v0.1.0-alpha.2 adds format and
codec research, measurements and archival scripts, without runtime changes.
The local project retains the media-derived vectors and experimental media in
Git-ignored `local-media/`; these are excluded from public release archives.
Host smoke and GitHub Ubuntu/macOS CI passed; no new hardware claim is made.
The Rust crate remains unimplemented. See ENCODER_LOG.md E055.

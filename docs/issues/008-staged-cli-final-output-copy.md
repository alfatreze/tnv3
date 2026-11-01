# 008 — Staged CLI final output copy is not atomic

**Status:** staged CLI only; not shipped. Host smoke-tested on macOS.

The CLI encodes to a short-named temporary sibling, opens that staged file,
then creates the requested final path with `create_new` and copies bytes. This
avoids overwriting an existing final path and works without hardlinks, which
keeps FAT/exFAT compatibility. The generated smoke test confirms valid C
checker output and that an existing output remains byte-for-byte unchanged.

The final filename is visible while copying. A process crash or power loss can
leave a partial final file. The staged file is reopened by pathname in the
output directory, so a concurrently writable shared directory permits a
replacement race between staging and reopen; cleanup by pathname also has a
race in such a directory. No shared-directory or FAT/exFAT test has run.

**Safe workaround:** use a private output directory, and inspect/remove any
partial output before retrying. Do not treat this staged CLI as shipped.

**Next gate:** test create-new, copy failure cleanup, crash residue, and
no-overwrite behavior on FAT and exFAT. Then harden staging against shared
directory replacement (for example, a private staging directory or a design
that retains an exclusive file handle) while preserving no-hardlink
compatibility, and rerun the platform tests before integration.

## Follow-up: exclusive publication prototype

**Host, 2026-11-01:** an experimental source-only `tau-codecs-cli` now uses
a private staging directory and directory-relative exclusive rename on
macOS/Linux, refuses group/world-writable output parents, and fails closed
when the filesystem cannot support the operation. It retains the pinned
parent-directory rename limitation described in its README. Generated macOS
smoke and no-overwrite tests passed. Windows intentionally returns unsupported;
Linux and FAT/exFAT runtime behavior are not yet verified. The prototype is
video-only and is not a complete cross-platform or Pocket-pair release.

## Pocket sidecar prototype

**Host, 2026-11-01:** the experimental Unix CLI can stage a TNV3 and MP3 and
publish each through exclusive no-replace rename. A generated macOS test
produced 24 decoded frames and 48 kHz stereo MP3; missing audio left no final
files, and existing output contents were preserved. Publication of two paths
is not atomic. Rollback is best effort and its identity check followed by
name-based unlink still permits a same-user race. Concurrent parent directory
rename also affects FFmpeg's path-based audio staging; see CLI README. No
Windows, Linux, card filesystem, or Pocket outcome is claimed.

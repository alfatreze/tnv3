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

## Windows MVP direct-copy path

**Cross-compile, 2026-11-01:** Windows uses `create_new` for final outputs
after staging and reports incomplete outputs rather than deleting them on
failure. This supplies an MVP path but final files can be visible while
copying and persist partially after errors or crashes. It assumes trusted
temp/output directories; ACL behavior, Windows runtime and FAT/exFAT have
not been verified. Manual inspection/removal may be needed after failure.

## ENOTSUP exFAT fallback and TauV verification

**Isolated implementation + host/card test, 2026-11-01:** an updated `tau-video-cli` source copy handles `ENOTSUP`/`EOPNOTSUPP` from directory-relative no-replace rename by exclusively creating the final file and copying/syncing from the private staged source. Ordinary rename is never used as a fallback. On errors, final paths are not unlinked; the CLI reports any partial final path and, for a pair, identifies when TNV3 remains after MP3 publication fails. Existing names remain protected by exclusive create. Copied final names are visible during transfer, so this path is explicitly non-atomic and can leave partial output after I/O failure or process/power loss.

Four new isolated tests passed for simulated unsupported rename, existing destination preservation, audio-name collision, and injected copy failure/partial-output reporting; the full isolated CLI suite passed 19/19. A real generated-media run on `/Volumes/TauV` (mounted exFAT, `noowners`) confirmed the filesystem returns unsupported for no-replace rename. The isolated CLI then wrote a generated 24-frame TNV3 (55,680 bytes, level 2) and matching MP3 (16,940 bytes) into a uniquely created test directory using the fallback. The same C reference toolchain checker, `tools/tnv3_ref/tnv3 check`, returned `valid` (a reference check, not an independent implementation); FFprobe reported MP3, 48 kHz stereo, 1.000-second duration. A generated collision case preserved pre-existing test audio and reported the remaining TNV3. Only generated test artifacts were removed; a read-only scan found no test/staging leftovers.

The filesystem test proves the tested macOS/exFAT create-new/copy/sync path for this CLI prototype; it does not prove other exFAT implementations, Windows/FAT behavior, crash durability, pair atomicity, independent full-container conformance, or Pocket playback/A/V synchronization. The fallback is now integrated into the CLI implementation. It remains an experimental prototype, not a release-qualified cross-platform path.

**Safe workaround:** use a trusted output directory; inspect the exact paths named by any failure and remove incomplete outputs manually before retrying. Do not treat a video-only remainder as a completed TNV3+MP3 pair.

**Next gate:** retain the no-unlink and no-replace guarantees, rerun the failure tests and generated TauV test against the integrated source, and keep Windows/FAT and Pocket playback gates separately open.

**Integrated host/card check:** the release CLI encoded a generated 24-frame
TNV3/MP3 pair on TauV exFAT using the fallback. The reference checker returned
valid, FFprobe reported 48 kHz stereo MP3, and a repeat attempt refused to
overwrite either output. The generated files and empty test directory were
removed. This confirms this workflow on the tested card, not Pocket playback
or general crash consistency.

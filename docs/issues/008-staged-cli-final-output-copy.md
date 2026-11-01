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

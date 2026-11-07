# TNV3 container preview check

`tau-codecs-cli check FILE --scope container-preview [--json]` runs the existing
cross-frame container checks, followed by subtitle structure validation, using
the same open reader. The CLI opens the path nonblocking with close-on-exec and
no-follow flags, then checks the opened descriptor with `fstat`; only regular
files proceed. FIFOs, devices, directories, symlinks, and unsupported targets
are `INDETERMINATE`. A stable regular file shorter than 64 bytes is `INVALID`.
`NOFOLLOW` protects the final path component; parent-directory symlinks may be
resolved by the operating system. Once opened, validation uses and stats that
same descriptor rather than reopening the path.
The input file must remain stable for the duration of the check. The first
finding stops the scan. The public Rust function accepts an already-opened
`Tnv3Reader`; path safety is the caller's responsibility.

The outcome is one of:

- `VALID`: both implemented container scopes completed without a finding.
- `INVALID`: malformed header, chunk/index/timing/interleaving/rate rules, or
  subtitle structure produced a typed finding.
- `INDETERMINATE`: an I/O error or an explicit implementation limit prevented
  the check from reaching a conclusion.

Exit codes are 0, 1, and 3 respectively. JSON includes `full_conformance_checked:
false` and `payload_decoding_checked:false` in every outcome. This check does
not decode TVC pictures or TAC audio frames and does not claim full TNV3 spec
conformance. The lower-level scopes remain available independently.

## Local reference-vector differential recipe

Keep licensed or large media outside the repository. Use the conformance vectors
in the local Tau Alpha project checkout, and run its C reference checker and
Python independent demuxer against the local codecs checkout. The command below uses a temporary
directory only for captured reports; it does not copy vectors or media.

```sh
CODECS_ROOT=/path/to/tau-codecs
TAU_ALPHA_ROOT=/path/to/tau-nu
cc -O2 -Wall -o /tmp/tnv3-reference "$TAU_ALPHA_ROOT/tools/tnv3_ref/tnv3.c"
python3 "$TAU_ALPHA_ROOT/tools/tnv3_ref/py/run_vectors.py"
cargo build --manifest-path "$CODECS_ROOT/tau-video-cli/Cargo.toml"
for f in "$TAU_ALPHA_ROOT"/tools/tnv3_ref/vectors/*.tnv3; do
  if /tmp/tnv3-reference check "$f" >/dev/null 2>&1; then ref=VALID; else ref=INVALID; fi
  if "$CODECS_ROOT/tau-video-cli/target/debug/tau-codecs-cli" check "$f" --scope container-preview --json > /tmp/tnv3-preview.json; then got=VALID; else rc=$?; if [ "$rc" -eq 1 ]; then got=INVALID; else got=INDETERMINATE; fi; fi
  printf '%s %s %s\n' "$f" "$ref" "$got"
done
```

The reference C checker is the acceptance oracle for TNV3 container validity;
Python `run_vectors.py` independently validates its vector accept/reject set.
Investigate every reference/Rust disagreement. An `INDETERMINATE` result is not
a rejection and must not be counted as a differential failure until the
resource or I/O cause is resolved. The recipe is intentionally local: it does
not upload, duplicate, or add source media to the repository.

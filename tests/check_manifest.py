#!/usr/bin/env python3
"""Verify pinned source-file SHA-256 values before building a release."""
import hashlib,json,pathlib
root=pathlib.Path(__file__).resolve().parents[1]
manifest=json.loads((root/'SOURCE_MANIFEST.json').read_text())
for name,entry in manifest['files'].items():
 p=root/name
 if hashlib.sha256(p.read_bytes()).hexdigest()!=entry['sha256']:raise SystemExit('source hash mismatch: '+name)
print('PASS',len(manifest['files']),'source-file hashes')

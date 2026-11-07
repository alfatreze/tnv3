# C tool versioning

`tvc_enc --version` and `tnv3 --version` report the independent tool CLI
version. The initial source version is 0.1.0; it is not the TVC or TNV3
format version. Change the appropriate tool version when releasing a change
to its command interface or behavior, following SemVer. A version does not
identify an exact source revision, compiler, build or set of supported format
drafts; record the executable SHA-256 when an exact file identity is needed.
The new endpoint is present only in binaries rebuilt from updated sources.
It is not a compatibility guarantee.

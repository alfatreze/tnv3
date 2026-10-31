# tau-video

A portable Rust library for the TNV3/TVC/TAC/TST family. The current
preliminary implementation reads and validates TNV3 headers and declared
index layout, and reads individual index entries through `Read + Seek`. It does
not validate chunks, subtitles or index consistency, and is not yet a TNV3
checker, decoder or encoder. Do not use it to certify files.

```sh
cargo test
cargo run --features cli -- info input.tnv3
```

Default features have no dependencies. Rust 1.85 is the minimum, matching the
minimum for Tau Omega's Rust 2024 edition. The library forbids unsafe code.
The caller supplies I/O; there is no UI, filesystem assumption or subprocess.

For local, non-redistributable reference vectors, set `TAU_VIDEO_VECTOR_DIR`
to the local `tools/tnv3_ref/vectors` folder under `local-media` before running
`cargo test`. Public tests use synthetic bytes. Header-derived lengths and
level limits are checked before use; index access allocates a fixed 16-byte
buffer. The present frame-count limit is 432,000 (four hours at 30 fps).
See `../docs/RUST_ROADMAP.md` and the consumer requirements for remaining work.

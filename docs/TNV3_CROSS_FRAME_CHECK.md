# TNV3 cross-frame check progress

The Rust crate provides `Tnv3Reader::check_cross_frame()` as an incremental,
process-free validation step. The method checks each picture through bounded
`read_picture()`, validates picture reference order, rebuilds display order,
checks presentation samples and cumulative TAC positions, and validates
key-frame `seek_tac`. The input must remain stable throughout the check because
reset prefixes are revisited after picture validation. This is an intermediate
validation layer, not a full TNV3 conformance checker.

The checker also applies the section 4.1 audio interleaving lower/upper bounds,
the section 4.2 rolling chunk-byte window, and the section 7.6 rolling picture
byte window. Window storage is fixed at the format's maximum 30 pictures.

The rules follow `docs/TNV3_CONTAINER_V0.md` sections 3–5 and were compared
with `tools/tnv3_ref/tnv3.c`'s `parse()` implementation. Tests run without
external tools or added media vectors: the checked-in synthetic TVC sequence
exercises display ordering and anchor failures, and generated audio payloads
exercise multiple reset points, seek indices, excess TAC counts, the final
chunk's upper-bound exemption, and both sides of the interleaving bounds.
Rate tests cover exact and one-byte-over limits for picture and chunk windows;
a 24,000/1,001 stream distinguishes the 24-picture ceiling window from a
23-picture window. The local C reference accepted nine locally held positive TNV3 vectors (not included in this repository).

Still outside this check: full TVC entropy/reference validation, TAC bitstream
decoding and audio sample accounting, and subtitle-area validation within this method. A separate bounded subtitle
structure check is available. The
selected structural checks do not establish complete format conformance.

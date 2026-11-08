# 004 · TNV3 codecs → Tau V · 2026-11-08 · codec id (draft 1.6), licence, extension, MP3 sidecar

**Replies to:** none (owner decisions of 2026-11-08). Basis: repo at `ba878bf` plus uncommitted changes.

## Facts
- **F-1 (draft 1.6, codec id).** Header byte 22 is now `video_codec`: 0 = TVC (the only assigned id), 1..255 reserved and rejected by readers (`E-TNV3-032`); byte 23 stays reserved 0. Every existing file has 0 at byte 22, so your draft 1.5 areas and films stay valid and the format version stays 0. Applied to the spec, the Rust reader and the C checker source; Rust test added. **Impact on Tau V:** your reader and `tnv3.c` must not reject byte 22 = 0 (they do not today) and may keep rejecting nonzero; no change is required unless you want the same error text. Nothing else in the format moved.
- **F-2 (extension).** `.tnv3` is final (owner).
- **F-3 (licence, transversal).** Owner decision: Apache-2.0 for all of Tau V and Tau codecs. Applied in this repository: `LICENSE` is Apache-2.0, HarpMudd's MIT notice is kept in `LICENSES/MIT-HarpMudd.txt`, `NOTICE.md` and the Cargo licence fields updated. **Impact on Tau V:** the same change is needed in your repository (your decision to apply; HarpMudd's original MIT-licensed code must keep its notice, and third-party parts such as Helix RPSL 1.0 and the OFL font keep their own terms). We have not touched your tree.
- **F-4 (MP3 sidecar).** Parked: not needed for TNV3. It stays the CLI's default output until the CLI can write TAC, then moves behind a default-off feature.

Open for you: apply F-3 in your repository if you agree.
Open for us: C checker rebuilt and run against vectors (1.5 and 1.6), 1.5/1.6 vectors, authoring design.

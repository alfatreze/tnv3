# Tau provenance and third-party notices

Tau is a derivative work based on
[HarpMudd MP3 Player](https://github.com/harpmudd/HarpMudd.mp3player) v1.4.0.
Development began from upstream commit
`7ef8f0fb84abd4ae5a6a187805fd910351ae57dc`.

Tau V preserves the upstream Git history. This extracted codec repository
starts from a source snapshot; `SOURCE_MANIFEST.json` records its provenance.
HarpMudd's original copyright notice remains in `LICENSES/MIT-HarpMudd.txt`,
alongside the notice for Tau's earlier modifications. Since 2026-11-08 Tau-authored code and
documentation in this repository is licensed under the Apache License 2.0 (`LICENSE`); HarpMudd's
MIT-licensed original work keeps its MIT terms, and neither replaces licenses attached
to third-party components. See `LICENSES/README.md`.

## Original project

- Project: HarpMudd MP3 Player for Analogue Pocket
- Author: HarpMudd
- Source: <https://github.com/harpmudd/HarpMudd.mp3player>
- Tau baseline: v1.4.0 / commit `7ef8f0fb84abd4ae5a6a187805fd910351ae57dc`
- License: MIT, preserved in `LICENSES/MIT-HarpMudd.txt`

## Tau

- Project direction, identity, artwork, packaging, documentation, and
  modifications after the baseline: alfatreze
- Source: <https://github.com/alfatreze/Tau-Alpha>
- License for Tau-authored code and documentation: Apache-2.0 (MIT before 2026-11-08)

## Rust dependencies of the subtitle builder

The optional `subtitle-builder` feature of `tau-video` uses the crates `ab_glyph` (Apache-2.0), `unicode-normalization` (MIT or Apache-2.0) and `regex` (MIT or Apache-2.0), each under its own licence.

## Components with separate terms

- Helix MP3 decoder: RealNetworks Public Source License 1.0. Its notices and
  license remain under `third_party/libhelix-mp3/`. Tau has made small, macro-gated
  modifications (decoder stage-cost profiling in `mp3dec.c` and `pub/mp3_profile.h`,
  and the `TAU_POLY_FW` hook in `real/dct32.c`, `real/subband.c` and `real/coder.h`
  that hands the synthesis-window stage to the FPGA unit); with those macros off
  the code is the unmodified decoder. See `docs/ATTRIBUTIONS.md`.
- Inter font and the generated font ROM derived from it: SIL Open Font License
  1.1, retained at `third_party/font/OFL.txt`.
- Analogue Pocket Framework files: their embedded Analogue APF license notices
  and Pocket EULA terms remain controlling for those files.
- Other vendored components retain their file-level MIT, ISC, or public-domain
  notices as described in the README credits.

No endorsement by HarpMudd, RealNetworks, Rasmus Andersson, Analogue, or other
upstream contributors is implied.

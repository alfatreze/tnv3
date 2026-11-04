# Notices

## Origin

This repository was extracted from the Tau V project (alfatreze/Tau-v, itself derived from
[Tau Alpha](https://github.com/alfatreze/Tau-Alpha), which began from
[HarpMudd MP3 Player](https://github.com/harpmudd/HarpMudd.mp3player) v1.4.0, commit
`7ef8f0fb84abd4ae5a6a187805fd910351ae57dc`). It starts from a source snapshot, not from the full upstream
history; `SOURCE_MANIFEST.json` records the provenance and hashes of the snapshot files. HarpMudd's original
copyright notice is kept in `LICENSE` next to the notice for later modifications. The MIT licence covers
HarpMudd's original work and the MIT-licensed additions of alfatreze; it does not replace the licences of the
third-party components below.

## What is in this repository

- Reference code and tools for TVC (video), TAC (audio) and TNV3 (container), a Rust crate (`tau-video`), a command-line
  tool and a desktop encoder application (`tau-encoder`). MIT.
- `tools/tst_ref/fonts/`: Roboto and UnifrakturCook fonts, used only to render subtitle test images; their licences are
  `OFL-Roboto.txt` and `OFL-UnifrakturCook.txt` in the same folder.
- No Helix MP3 decoder, no Inter font and no Analogue Pocket framework files: those belong to the player project (Tau V)
  and are not in this repository.

## Separately licensed software built into the macOS application bundle

`tau-encoder/scripts/build-ffmpeg-macos.sh` downloads and builds, at packaging time, **FFmpeg n9.0.2** and **LAME 4.0**
(`libmp3lame`) as helper programs of the application bundle. FFmpeg is configured without `--enable-gpl`, so it is used
under the GNU LGPL v2.1 or later; LAME is licensed under the LGPL. The script copies the licence texts
(`FFmpeg-COPYING.LGPLv2.1`, `LAME-COPYING`) and the exact source archives (checked against pinned SHA-256 values)
into the bundle next to the binaries. None of their source is stored in this repository. Anyone redistributing a bundle
must keep those files with it.

## Test material

Reference vectors derived from film or television sources, and the research media, are never published; they stay in
the git-ignored `local-media/`. Tracked fixtures are synthetic.

No endorsement by HarpMudd, FFmpeg, LAME, Analogue or other upstream authors is implied.

# Tau Encoder logo

`tau-encoder-logo.svg` is the source: concept 8 of `concepts/` (a tau inside an encode-progress ring) in the green accent of Tau Omega (`#c1f0ad`, secondary `#669c83`, tile `#111617`). It replaced the first, amber proposal (a film-strip tau), after the owner chose icon 8.

`tau-encoder-icon-1024.png` is the 1024 px render with transparent corners. The app icons in `src-tauri/icons/` (`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`) were generated from it with `cargo tauri icon`; `public/tau-encoder-logo.svg` is the copy shown in the app's sidebar. Regenerate with:

    cargo tauri icon branding/tau-encoder-icon-1024.png -o /tmp/icons   # then copy the five files above

Licence: Apache-2.0 like the rest of the repository (original artwork by this project).

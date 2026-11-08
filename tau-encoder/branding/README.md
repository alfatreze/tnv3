# Tau Encoder logo

`tau-encoder-logo.svg` is the source: an amber tau (a film-strip bar over a stem whose three dashes shrink, for compression) on a dark navy tile. It is deliberately different from the green pulse icon of Tau Omega.

`tau-encoder-icon-1024.png` is the 1024 px render with transparent corners. The app icons in `src-tauri/icons/` (`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`) were generated from it with `cargo tauri icon`; `public/tau-encoder-logo.svg` is the copy shown in the app's sidebar. Regenerate with:

    cargo tauri icon branding/tau-encoder-icon-1024.png -o /tmp/icons   # then copy the five files above

Licence: Apache-2.0 like the rest of the repository (original artwork by this project).

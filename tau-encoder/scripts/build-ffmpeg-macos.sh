#!/bin/sh
# Build separately licensed FFmpeg and LAME helpers for a macOS application bundle.
set -eu
case "$(uname -s)" in Darwin) ;; *) echo 'This helper builds on macOS only.' >&2; exit 1;; esac
appdir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d /tmp/tau-ffmpeg.XXXXXXXX)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
if [ -n "${TAU_FFMPEG_ARCHIVE:-}" ]; then cp "$TAU_FFMPEG_ARCHIVE" ffmpeg-n9.0.2.tar.gz; else curl -fL https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n9.0.2.tar.gz -o ffmpeg-n9.0.2.tar.gz; fi
if [ -n "${TAU_LAME_ARCHIVE:-}" ]; then cp "$TAU_LAME_ARCHIVE" lame-4.0.tar.gz; else curl -fL https://downloads.sourceforge.net/project/lame/lame/4.0/lame-4.0.tar.gz -o lame-4.0.tar.gz; fi
printf '%s  %s\n' 6e374ed621e48faa40639307dff48ba6fe574a509977956d2cce9669b7cc27e9 ffmpeg-n9.0.2.tar.gz | shasum -a 256 -c -
printf '%s  %s\n' 3df5124d5ad3a98312ffd7ba6a9b36230e4f8a3e66d3ce0f425e336c32d216eb lame-4.0.tar.gz | shasum -a 256 -c -
tar xzf ffmpeg-n9.0.2.tar.gz
tar xzf lame-4.0.tar.gz
cd lame-4.0
ac_cv_prog_cc_c23=no CFLAGS='-O2 -Wno-implicit-function-declaration' ./configure --prefix="$work/install" --disable-decoder --disable-frontend --disable-static
make -j"$(sysctl -n hw.ncpu)"
make install
cd "$work/FFmpeg-n9.0.2"
PKG_CONFIG_PATH="$work/install/lib/pkgconfig" ./configure --disable-autodetect --disable-shared --enable-static --enable-libmp3lame --disable-doc --disable-ffplay "--extra-cflags=-I$work/install/include" "--extra-ldflags=-L$work/install/lib"
make -j"$(sysctl -n hw.ncpu)" ffmpeg ffprobe
out="$appdir/src-tauri/resources/ffmpeg"
mkdir -p "$out"
rm -f "$out/ffmpeg-n6.1.1.tar.gz"
cp ffmpeg ffprobe "$out/"
cp "$work/install/lib/libmp3lame.0.dylib" "$out/"
install_name_tool -change "$work/install/lib/libmp3lame.0.dylib" '@loader_path/libmp3lame.0.dylib' "$out/ffmpeg"
install_name_tool -change "$work/install/lib/libmp3lame.0.dylib" '@loader_path/libmp3lame.0.dylib' "$out/ffprobe"
install_name_tool -id '@rpath/libmp3lame.0.dylib' "$out/libmp3lame.0.dylib"
cp "$work/ffmpeg-n9.0.2.tar.gz" "$work/lame-4.0.tar.gz" "$out/"
cp COPYING.LGPLv2.1 "$out/FFmpeg-COPYING.LGPLv2.1"
cp "$work/lame-4.0/COPYING" "$out/LAME-COPYING"
cp "$appdir/scripts/build-ffmpeg-macos.sh" "$out/BUILD.sh"

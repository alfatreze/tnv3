#!/bin/sh
# Build separately licensed FFmpeg and LAME helpers for a macOS application bundle.
set -eu
case "$(uname -s)" in Darwin) ;; *) echo 'This helper builds on macOS only.' >&2; exit 1;; esac
appdir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d /tmp/tau-ffmpeg.XXXXXXXX)
trap 'rm -rf "$work"' EXIT HUP INT TERM
cd "$work"
curl -fL https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n6.1.1.tar.gz -o ffmpeg-n6.1.1.tar.gz
curl -fL https://downloads.sourceforge.net/project/lame/lame/4.0/lame-4.0.tar.gz -o lame-4.0.tar.gz
printf '%s  %s\n' 7c1ebea95d815e49c1e60c7ee816410dec73a81b8ac002b276780d2f9048e598 ffmpeg-n6.1.1.tar.gz | shasum -a 256 -c -
printf '%s  %s\n' 3df5124d5ad3a98312ffd7ba6a9b36230e4f8a3e66d3ce0f425e336c32d216eb lame-4.0.tar.gz | shasum -a 256 -c -
tar xzf ffmpeg-n6.1.1.tar.gz
tar xzf lame-4.0.tar.gz
cd lame-4.0
ac_cv_prog_cc_c23=no CFLAGS='-O2 -Wno-implicit-function-declaration' ./configure --prefix="$work/install" --disable-decoder --disable-frontend --disable-static
make -j"$(sysctl -n hw.ncpu)"
make install
cd "$work/FFmpeg-n6.1.1"
PKG_CONFIG_PATH="$work/install/lib/pkgconfig" ./configure --disable-autodetect --disable-shared --enable-static --enable-libmp3lame --disable-doc --disable-ffplay --disable-ffprobe "--extra-cflags=-I$work/install/include" "--extra-ldflags=-L$work/install/lib"
make -j"$(sysctl -n hw.ncpu)" ffmpeg
out="$appdir/src-tauri/resources/ffmpeg"
mkdir -p "$out"
cp ffmpeg "$out/ffmpeg"
cp "$work/install/lib/libmp3lame.0.dylib" "$out/"
install_name_tool -change "$work/install/lib/libmp3lame.0.dylib" '@loader_path/libmp3lame.0.dylib' "$out/ffmpeg"
install_name_tool -id '@rpath/libmp3lame.0.dylib' "$out/libmp3lame.0.dylib"
cp "$work/ffmpeg-n6.1.1.tar.gz" "$work/lame-4.0.tar.gz" "$out/"
cp COPYING.LGPLv2.1 "$out/FFmpeg-COPYING.LGPLv2.1"
cp "$work/lame-4.0/COPYING" "$out/LAME-COPYING"
cp "$appdir/scripts/build-ffmpeg-macos.sh" "$out/BUILD.sh"

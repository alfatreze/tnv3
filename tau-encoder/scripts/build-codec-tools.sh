#!/bin/sh
set -eu
appdir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
root=$(CDPATH= cd -- "$appdir/.." && pwd)
out="$appdir/src-tauri/resources/codec-tools"
mkdir -p "$out/tnv3_ref" "$out/tvc_ref"
cp "$root/LICENSE" "$out/LICENSE"
cp "$root/tools/tnv3_ref/encode_film.py" "$out/tnv3_ref/"
cc -O3 -o "$out/tnv3_ref/tnv3" "$root/tools/tnv3_ref/tnv3.c" -lm
cc -O3 -o "$out/tvc_ref/tvc_enc" "$root/tools/tvc_ref/tvc_enc.c" -lm

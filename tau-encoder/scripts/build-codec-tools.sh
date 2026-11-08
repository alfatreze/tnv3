#!/bin/sh
set -eu
appdir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
root=$(CDPATH= cd -- "$appdir/.." && pwd)
out="$appdir/src-tauri/resources/codec-tools"
mkdir -p "$out/tnv3_ref" "$out/tvc_ref" "$out/tac_ref"
cp "$root/LICENSE" "$out/LICENSE"
cp "$root/tools/tnv3_ref/encode_film.py" "$out/tnv3_ref/"
cc -O3 -o "$out/tnv3_ref/tnv3" "$root/tools/tnv3_ref/tnv3.c" -lm
cc -O3 -o "$out/tvc_ref/tvc_enc" "$root/tools/tvc_ref/tvc_enc.c" -lm
cc -O3 -o "$out/tac_ref/tac" "$root/tools/tac_ref/tac.c" -lm
cc -O3 -o "$out/tac_ref/tac_fx" "$root/tools/tac_ref/tac_fx.c" -lm

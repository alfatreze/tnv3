#!/bin/sh
# End-to-end frame decode of one study stream (stream -> headers -> contexts -> symbols), see sim/tb_tau_av1_frame.v.
#   tools/codec_study/run_frame.sh NAME [TILES]      e.g. dialog 30   (needs work/codec_phase0/av1/NAME_lean2.{ivf,blk}, build/cdf_NAME_lean2.txt
#   from sim/av1_cdfctx.py, build/hdr_dump, and the testbench built once:  make build/rtl/tb_av1_frame.vvp)
cd "$(dirname "$0")/../.."
n=$1; t=${2:-30}
python3 sim/av1/cdfctx_vectors.py build/rtl > /dev/null
python3 sim/av1/frame_vectors.py work/codec_phase0/av1/${n}_lean2.ivf work/codec_phase0/av1/${n}_lean2.blk build/cdf_${n}_lean2.txt build/rtl/fr_$n $t > /dev/null
echo "$n: $(vvp build/rtl/tb_av1_frame.vvp +vec=build/rtl/fr_$n 2>&1 | grep -m1 'PASSED\|FAILED')"

#!/bin/sh
# Real-bytes decode check of one study stream: trace -> word stream (first N frames) -> tau_av1_blk + tau_av1_syn + tau_av1_coef.
#   tools/codec_study/run_int.sh NAME [FRAMES]       e.g. dialog 30   (needs work/codec_phase0/av1/NAME_lean2.blk from blk_traces.py)
# Build the testbench once:  iverilog -g2012 -I src/fpga/core -Ptb_tau_av1_int.NTILE=32 -Ptb_tau_av1_int.MEMWORDS=6000000 \
#   -o build/rtl/tb_av1_int32.vvp sim/tb_tau_av1_int.v src/fpga/core/tau_av1_{blk,syn,symdec,coef}.sv
cd "$(dirname "$0")/../.."
n=$1; f=${2:-30}
python3 sim/av1/blk_vectors.py work/codec_phase0/av1/${n}_lean2.blk build/rtl/int_$n.hex --inter --frames $f > /dev/null
echo "$n: $(vvp build/rtl/tb_av1_int32.vvp +vec=build/rtl/int_$n.hex 2>&1 | grep -m1 'PASSED\|FAILED')"

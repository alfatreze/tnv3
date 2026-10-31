# AV1-lean inter path: what is missing and the plan (V-093 follow-up)

State at 2026-10-22: the intra pipeline is complete and verified (block syntax, coefficients, reconstruction, deblocking, frame store, colour output, registers, driver; see V-088..V-093).
Inter frames decode their SYNTAX in hardware (every inter frame of the committed streams passes `make test-rtl-av1-frame`: modes, reference frames, motion vectors, transform
sizes and coefficients equal libaom's) but nothing reconstructs an inter block yet. Everything below is new; none of it is built.

## What an inter block needs (the lean tool set: no warp, no OBMC, no inter-intra, no masked / wedge / distance-weighted compound, no scaled references, no intra block copy)

1. **Prediction jobs.** libaom builds the prediction of a whole block per plane before any residual is added (`dec_build_inter_predictor`: for every plane, `build_inter_predictors`).
   One job per (plane, reference): destination position and size in the plane, the motion vector (1/8 luma sample units), the reference frame slot, the interpolation filter
   (one filter per block, both directions, since dual filter is off). Chroma blocks of 4xN / Nx4 luma blocks (`is_sub8x8_inter`) are built from the motion vectors of up to four
   neighbouring luma blocks (each 2x2 / 2x4 / 4x2 chroma piece with its own MV and reference): the control needs the neighbours' MVs, i.e. the mode-info ring of `tau_av1_blk`
   must be readable by the reconstruction control (today it only goes the other way).
2. **Reference window fetch.** For a block (w x h) at plane position (x, y) with the motion vector scaled to 1/16 samples (luma MV x 2, chroma MV x 1 for 4:2:0) the filter needs
   (w + 7) x (h + 7) reference samples with the coordinates clamped to the frame (edge replication; libaom additionally clamps the MV so the block stays within a border, which does
   not change the result because everything beyond is replicated). `tau_av1_mc2` (verified against libaom's convolve) takes sample streams per lane and does the 8-tap filters and
   the compound average; a window streamer (row bursts from the frame store, clamp, raster order) is missing.
3. **Residual.** After the prediction the transform blocks are added in the order of the coefficient parse (variable transform tree for inter blocks): per transform block the
   prediction is read back from the superblock buffer (`tau_av1_recon_buf` cur), the residual from `tau_av1_itx` is added (`tau_av1_recon_add`) and the sum written back. The engine
   today predicts per transform block (intra); the inter flow is block prediction first, then residual per transform block. `tau_av1_blk` has to tell the engine where each
   transform block is (plane, position, size) when it starts its coefficients (today the jobs come from `tau_av1_recon_ctl` / `seq`, which derive the intra transform blocks).
4. **References.** After deblocking, a frame is a reference: 9 frame buffers in the SDRAM (the driver already plans the 9 context blocks the same way), the seven reference
   frame types point at buffers through `ref_frame_idx`; the driver programs seven base addresses. The unit never writes a buffer that is still referenced.
5. **Deblocking of inter frames.** The walker's per-unit map needs, besides the transform size, skip, intra / inter, reference frame, prediction mode and the block size
   (`tau_av1_lf_edge` already takes all of them; the model `lf_model.h` is checked against libaom with them).
6. **Intra blocks inside inter frames** are unchanged (their neighbours include inter blocks, availability already handles that).

## Verification plan (same recipe as every block: model from the specification, vectors from libaom, mutants)

* C0 oracle: `AV1_INTER_TRACE` in the scratch libaom patch: per `build_one_inter_predictor` call the plane position, size, MV, reference frame and slot, filter, compound flag and
  the predicted samples of the plane after `dec_build_inter_predictor` (so sub8x8 chroma and compound are covered); reference frames come from the loop-filter trace (post-filter planes).
* C1 predictor: model (C) + RTL `tau_av1_interpred` = window streamer + `tau_av1_mc2`; test against the trace with a modelled reference frame store.
* C2 control: jobs for inter blocks from the block events (+ neighbour MVs), residual jobs from the coefficient-start events; test = the whole frame (intra frame + inter frames)
  equals libaom's frames before deblocking, then after deblocking.
* C3 driver / registers: reference slots, buffer management, one more register group; replay test with a two-frame stream.

Cost estimate (to be measured): the inter prediction needs about (w + 7)(h + 7) sample reads per block per reference; a 400x176 frame is about 70 k luma + 35 k chroma samples
predicted with a 1.3x window overhead per reference: ~0.15 M clocks single, ~0.3 M compound at one sample per clock (estimate), small next to the symbol decoding (about 0.3 M
clocks per P frame in the committed streams' symbol counts: to be measured).

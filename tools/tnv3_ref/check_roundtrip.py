#!/usr/bin/env python3
"""End-to-end check of the positive TNV3 vectors with the C reference tools: the demuxed video decodes (tvc_dec) to the
same pictures as the TVC source stream, and the demuxed audio decodes (tac dec) to a prefix of the TAC source's output
(the vectors muxed with -T drop trailing TAC frames). Needs tools/tnv3_ref/tnv3, tools/tvc_ref/tvc_dec, tools/tac_ref/tac."""
import os, subprocess, sys, tempfile, wave

ROOT = os.path.dirname(os.path.abspath(__file__))
T = lambda *p: os.path.join(ROOT, '..', *p)
SOURCES = {   # vector -> (TVC source, TAC source or None)
    'p01_ip_24_crc': ('v01_dialog_ip', 't02_film_action_96k'),
    'p02_b2_23976_crc': ('v06_dialog_b2_tree', 't02_film_action_96k'),
    'p03_b3_gop7_2997': ('v07_anime_b3_gop7', 't03_film_dialog_128k'),
    'p04_slices_25_mono': ('v16_dialog_l2_3slices', 't04_tv_mono_64k'),
    'p05_l1_video_only': ('v19_l1_crop_slices', None),
    'p06_crop_b2_subs': ('v09_odd_crop_b2', 't01_synth_stereo_96k'),
    'p07_long_audio_tail': ('v07_anime_b3_gop7', 't05_synth_lr_dense_clicks'),
    'p09_seek_at_reset': ('v01_dialog_ip', 't02_film_action_96k'),
    'p08_l3_2slices_30': ('v17_bw_l3_2slices', 't06_synth_ms_all_320k'),
}


def sh(*a):
    r = subprocess.run(list(a), capture_output=True)
    if r.returncode:
        sys.exit('%s failed: %s' % (a[0], r.stderr.decode()))


def frames(y4m):
    b = open(y4m, 'rb').read()
    return b[b.index(b'\n') + 1:]   # drop the stream header (it carries the frame rate)


def pcm(path):
    with wave.open(path) as w:
        return w.getnchannels(), w.readframes(w.getnframes())


tmp = tempfile.mkdtemp()
fails = 0
for name, (v, a) in SOURCES.items():
    f = os.path.join(ROOT, 'vectors', name + '.tnv3')
    o = lambda e: os.path.join(tmp, name + e)
    sh(T('tnv3_ref', 'tnv3'), 'demux', f, o('.tvc'), o('.tac'))
    sh(T('tvc_ref', 'tvc_dec'), o('.tvc'), o('.y4m'))
    sh(T('tvc_ref', 'tvc_dec'), T('tvc_ref', 'vectors', v + '.tvc'), o('.src.y4m'))
    ok = frames(o('.y4m')) == frames(o('.src.y4m'))
    msg = 'video same' if ok else 'VIDEO DIFFERS'
    if a:
        sh(T('tac_ref', 'tac'), 'dec', o('.tac'), o('.wav'))
        sh(T('tac_ref', 'tac'), 'dec', T('tac_ref', 'vectors', a + '.tac'), o('.src.wav'))
        (c1, p1), (c2, p2) = pcm(o('.wav')), pcm(o('.src.wav'))
        aok = c1 == c2 and len(p1) <= len(p2) and p1 == p2[:len(p1)]
        msg += ', audio %s (%d of %d samples)' % ('prefix' if aok else 'DIFFERS', len(p1) // 2 // c1, len(p2) // 2 // c2)
        ok = ok and aok
    print(name, 'PASS' if ok else 'FAIL', msg)
    fails += not ok
sys.exit(1 if fails else 0)

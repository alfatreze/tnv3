"""Pack and unpack the TNV3 subtitle area (docs/TNV3_CONTAINER_V0.md section 6.1, draft 1.4).

A track is dict(lang='eng', default=False, forced_track=False, variants=[(event_list_bytes, glyph_table_bytes), ...]),
variants in increasing cell height; cell_w / cell_h are read from each glyph table. Optional key 'palette': 16 (r, g, b)
tuples (entry 0 = ordinary dialogue; default DEFAULT_PALETTE)."""
DEFAULT_PALETTE = [(255, 255, 255), (255, 230, 0), (0, 230, 255), (0, 230, 0)] + [(160, 160, 160)] * 12
import struct


def _align4(x):
    return (x + 3) & ~3


def pack_area(tracks):
    if not 1 <= len(tracks) <= 15:
        raise ValueError('1..15 tracks')
    dir_len = 4 + sum(56 + 20 * len(t['variants']) for t in tracks)
    pos = _align4(dir_len)
    body = bytearray(pos - dir_len)            # padding after the directory
    entries = bytearray(struct.pack('<B3x', len(tracks)))
    for t in tracks:
        lang = t['lang'].encode('ascii')
        if not (2 <= len(lang) <= 4 and lang.isalpha() and lang.islower()):
            raise ValueError('language code: %r' % t['lang'])
        nv = len(t['variants'])
        if not 1 <= nv <= 8:
            raise ValueError('1..8 variants')
        flags = (1 if t.get('default') else 0) | (2 if t.get('forced_track') else 0)
        pal = t.get('palette') or DEFAULT_PALETTE
        if len(pal) != 16:
            raise ValueError('palette needs 16 entries')
        entries += lang.ljust(4, b'\0') + struct.pack('<BBxx', flags, nv) + bytes(c for rgb in pal for c in rgb)
        for ev, gl in t['variants']:
            eo = pos
            body += ev
            pos += len(ev)
            pad = _align4(pos) - pos
            body += bytes(pad); pos += pad
            go = pos
            body += gl
            pos += len(gl)
            cw, ch = gl[2], gl[3]
            entries += struct.pack('<BBHIIII', cw, ch, 0, eo, len(ev), go, len(gl))
            pad = _align4(pos) - pos
            body += bytes(pad); pos += pad
    assert len(entries) == dir_len
    return (bytes(entries) + bytes(body))[:go + len(gl)]     # no padding after the last glyph table


def unpack_area(a):
    """-> list of tracks as in pack_area (no validation; tnv3 check and tnv3_demux.py validate)"""
    nt, pos, tracks = a[0], 4, []
    for _ in range(nt):
        lang = a[pos:pos + 4].rstrip(b'\0').decode('ascii')
        flags, nv = a[pos + 4], a[pos + 5]
        pal = [tuple(a[pos + 8 + 3 * k:pos + 11 + 3 * k]) for k in range(16)]
        vs = []
        for v in range(nv):
            cw, ch, _, eo, el, go, gl = struct.unpack_from('<BBHIIII', a, pos + 56 + 20 * v)
            vs.append((bytes(a[eo:eo + el]), bytes(a[go:go + gl])))
        tracks.append(dict(lang=lang, default=bool(flags & 1), forced_track=bool(flags & 2), palette=pal, variants=vs))
        pos += 56 + 20 * nv
    return tracks

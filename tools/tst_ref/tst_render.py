#!/usr/bin/env python3
"""TST reference renderer: the subtitle state at time T as a PNG of the 800x720 output window, with the player's
display parameters of container section 6.4 (docs/TNV3_CONTAINER_V0.md, docs/TST_SUBTITLES_V0.md).

    tst_render.py FILE.tnv3|AREA.bin [--video WxH] --time SECONDS -o OUT.png [--frame IMG.png] [--panel]
                  [--preset NAME] [--track eng|N|off] [--size N|WxH] [--subs on|off] [--hide-sdh] [--delay MS]
                  [--voffset ROWS] [--text-color RRGGBB] [--override-colors] [--outline-color RRGGBB]
                  [--outline-width 0..4] [--box-color RRGGBB] [--box-pad 0..16]
    tst_render.py FILE --time S --sheet OUT.png [--presets a,b,c]      (contact sheet, one cell per preset)

Command-line values override the preset (tools/tst_ref/presets.json), which overrides the defaults below.
Layers, bottom to top: picture, box, outline, text. Everything is drawn at full strength; the 2-bit coverage
0..3 means 0, 1/3, 2/3, 1 of the layer colour. Colours come from the track's 16-entry palette; --text-color replaces entry 0 only, --override-colors draws every
entry in the text colour. Pillow only."""
import argparse
import json
import os
import struct
import sys

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, '..', 'tnv3_ref', 'py'))
import tnv3_demux  # noqa: E402
from tst_area import unpack_area  # noqa: E402

GRID_W, GRID_H = 800, 720
BREAK = 0xFFFF
SWITCH = 0xFFE0
RATE = 48000


class RenderError(Exception):
    pass


DEFAULTS = dict(
    track=None,            # None = default track (else track 0); a language code, an index, or 'off'
    size=None,             # None = middle variant; an index (negative counts from the largest) or 'WxH'
    subs='on',             # 'on' | 'off' (off: only forced events)
    hide_sdh=False,
    delay=0,               # ms; positive = subtitles later
    voffset=0,             # grid rows added to every event's y
    text_color=None,       # None = the track's palette entry 0; else RRGGBB replacing entry 0
    override_colors=False, # True: every palette entry is drawn in the text colour
    outline_color='000000', outline_width=0,
    box_color=None,        # None = no box
    box_pad=4,
)
PRESET_FILE = os.path.join(HERE, 'presets.json')


def load_presets(path=PRESET_FILE):
    with open(path) as f:
        return json.load(f)


def hex_rgb(s):
    s = str(s).lstrip('#')
    if len(s) != 6:
        raise RenderError('colour must be RRGGBB: %r' % s)
    return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))


def resolve(preset=None, overrides=None, presets=None):
    p = dict(DEFAULTS)
    if preset:
        presets = presets if presets is not None else load_presets()
        if preset not in presets:
            raise RenderError('unknown preset %r (have: %s)' % (preset, ', '.join(presets)))
        for k, v in presets[preset].items():
            if k.startswith('_'):
                continue
            if k not in DEFAULTS:
                raise RenderError('preset %s: unknown parameter %s' % (preset, k))
            p[k] = v
    for k, v in (overrides or {}).items():
        if v is not None:
            p[k] = v
    if not 0 <= int(p['outline_width']) <= 4:
        raise RenderError('outline width 0..4')
    if not 0 <= int(p['box_pad']) <= 16:
        raise RenderError('box padding 0..16')
    return p


# --------------------------------------------------------------------------------------------- loading

def parse_variant(ev, gl):
    """-> (events, glyphs, cw, ch); events = (start, end, x, y, colour, flags, ids); glyphs = (advance, rows)."""
    gc, cw, ch = struct.unpack('<HBB', gl[:4])
    rowb = (cw + 3) // 4
    glyphs, p = [], 4
    for _ in range(gc):
        adv = gl[p]
        p += 1
        rows = []
        for _ in range(ch):
            rows.append([(gl[p + x // 4] >> (2 * (x % 4))) & 3 for x in range(cw)])
            p += rowb
        glyphs.append((adv, rows))
    count = struct.unpack('<I', ev[:4])[0]
    p, events = 4, []
    for _ in range(count):
        st, en, x, y, col, fl, n = struct.unpack('<IIHHBBH', ev[p:p + 16])
        p += 16
        ids = list(struct.unpack('<%dH' % n, ev[p:p + 2 * n]))
        p += 2 * n
        events.append((st, en, x, y, col, fl, ids))
    return events, glyphs, cw, ch


def load_input(path, video=None):
    """-> (tracks, (vw, vh)); tracks as tst_area.unpack_area. The area is validated with the container checker."""
    with open(path, 'rb') as f:
        data = f.read()
    if path.endswith('.tnv3'):
        try:
            info = tnv3_demux.parse(data)
        except tnv3_demux.TnvError as e:
            raise RenderError('%s: %s' % (path, e))
        if info['subs'] is None:
            raise RenderError('file has no subtitles')
        area, size = info['subs'], (info['width'], info['height'])
        if video:
            size = video
    else:
        try:
            tnv3_demux.check_area(data)
        except tnv3_demux.TnvError as e:
            raise RenderError('%s: not a valid subtitle area: %s' % (path, e))
        if not video:
            raise RenderError('an area file needs --video WxH')
        area, size = data, video
    return unpack_area(area), size


def select(tracks, p):
    """Track and variant selection (6.4). -> (track, variant_index, forced_only)."""
    want = p['track']
    off = p['subs'] == 'off' or want == 'off'
    if want in (None, 'off'):
        base = next((t for t in tracks if t['default']), tracks[0])
    elif str(want).isdigit():
        if int(want) >= len(tracks):
            raise RenderError('track %s: file has %d tracks' % (want, len(tracks)))
        base = tracks[int(want)]
    else:
        base = next((t for t in tracks if t['lang'] == want and not t['forced_track']), None) or \
            next((t for t in tracks if t['lang'] == want), None)
        if base is None:
            raise RenderError('no track with language %r (have: %s)' % (want, ', '.join(t['lang'] for t in tracks)))
    sz = p['size']
    nv = len(base['variants'])
    cells = [(v[1][2], v[1][3]) for v in base['variants']]
    if sz is None:
        vi = nv // 2
    elif isinstance(sz, str) and 'x' in sz.lower():
        cw, ch = (int(v) for v in sz.lower().split('x'))
        if (cw, ch) not in cells:
            raise RenderError('no variant %s (have %s)' % (sz, ', '.join('%dx%d' % c for c in cells)))
        vi = cells.index((cw, ch))
    else:
        vi = int(sz)
        if not -nv <= vi < nv:
            raise RenderError('size %d: track has %d variants' % (vi, nv))
        vi %= nv
    if not off:
        return base, vi, False
    # subtitles off: the forced track of the same language if there is one, else the selected track itself
    ft = next((t for t in tracks if t['forced_track'] and t['lang'] == base['lang']), base)
    ch = cells[vi][1]
    fh = [v[1][3] for v in ft['variants']]
    fi = min(range(len(fh)), key=lambda i: (abs(fh[i] - ch), i))     # nearest cell height
    return ft, fi, True


# --------------------------------------------------------------------------------------------- drawing

def place(ev, glyphs, ch):
    """Yield (gid, x, y, colour) for every glyph cell of an event (the drawing rule of 6.3): the colour starts as the
    event's colour, a switch 0xFFE0 + k changes it (it persists across line breaks, the pen does not move)."""
    x0, y0, ids = ev[2], ev[3], ev[6]
    x, y, col = x0, y0, ev[4]
    for g in ids:
        if g == BREAK:
            x, y = x0, y + ch
        elif g >= SWITCH:
            col = g - SWITCH
        else:
            yield g, x, y, col
            x += glyphs[g][0]


def event_box(ev, glyphs, cw, ch):
    """Box rectangle of an event, exclusive end: from the event origin to the pen extent of its longest line
    (sum of advances) horizontally, and all its lines vertically (see docs/TST_SUBTITLES_V0.md)."""
    x0, y0, ids = ev[2], ev[3], ev[6]
    x, lines, wmax = x0, 1, 0
    for g in ids:
        if g == BREAK:
            x, lines = x0, lines + 1
        elif g < SWITCH:
            x += glyphs[g][0]
            wmax = max(wmax, x - x0)
    if wmax == 0:
        return None
    return (x0, y0, x0 + wmax, y0 + lines * ch)


def dilate(cov, w):
    """Grey-scale dilation of a sparse coverage map by a disc of radius w (grid pixels within distance w)."""
    if w == 0:
        return dict(cov)
    offs = [(dx, dy) for dx in range(-w, w + 1) for dy in range(-w, w + 1) if dx * dx + dy * dy <= w * w]
    out = {}
    for (x, y), a in cov.items():
        for dx, dy in offs:
            q = (x + dx, y + dy)
            if 0 <= q[0] < GRID_W and 0 <= q[1] < GRID_H and out.get(q, 0) < a:
                out[q] = a
    return out


def background(video, frame=None):
    img = Image.new('RGB', (GRID_W, GRID_H), (0, 0, 0))
    vw, vh = video
    top, left = (GRID_H - vh) // 2, (GRID_W - vw) // 2
    if frame:
        pic = Image.open(frame).convert('RGB')
        if pic.size != (vw, vh):
            pic = pic.resize((vw, vh), Image.LANCZOS)
        img.paste(pic, (left, top))
    else:
        img.paste((110, 110, 110), (left, top, left + vw, top + vh))
    return img


def render(tracks, p, video, t_samples, frame=None, base=None):
    """The 800x720 image at output position t_samples (before the delay setting) with parameters p."""
    img = (base or background(video, frame)).copy()
    trk, vi, forced_only = select(tracks, p)
    events, glyphs, cw, ch = parse_variant(*trk['variants'][vi])
    pos = t_samples - int(round(p['delay'] * RATE / 1000))
    voff = int(p['voffset'])
    act = []
    for ev in events:
        if not ev[0] <= pos < ev[1]:
            continue
        if forced_only and not ev[5] & 1:
            continue
        if p['hide_sdh'] and ev[5] & 2:
            continue
        act.append((ev[0], ev[1], ev[2], ev[3] + voff, ev[4], ev[5], ev[6]))
    cov = {}
    for ev in act:                      # coverage map: larger alpha wins, ties keep the earlier event
        for g, x, y, colr in place(ev, glyphs, ch):
            for r, row in enumerate(glyphs[g][1]):
                yy = y + r
                if not 0 <= yy < GRID_H:
                    continue
                for c, a in enumerate(row):
                    xx = x + c
                    if a and 0 <= xx < GRID_W and cov.get((xx, yy), (0, 0))[0] <= a:   # later glyph wins a tie
                        cov[(xx, yy)] = (a, colr)
    # box: solid union of the per-event rectangles grown by the padding
    if p['box_color'] is not None:
        pad = int(p['box_pad'])
        mask = Image.new('L', (GRID_W, GRID_H), 0)
        d = ImageDraw.Draw(mask)
        for ev in act:
            b = event_box(ev, glyphs, cw, ch)
            if b:
                d.rectangle((b[0] - pad, b[1] - pad, b[2] + pad - 1, b[3] + pad - 1), fill=255)
        img.paste(hex_rgb(p['box_color']), mask=mask)
    px = img.load()

    def blend(layer, rgb):
        for (x, y), a in layer.items():
            b = px[x, y]
            px[x, y] = tuple((b[i] * (3 - a) + rgb[i] * a + 1) // 3 for i in range(3))
    w = int(p['outline_width'])
    if w:
        blend(dilate({k: v[0] for k, v in cov.items()}, w), hex_rgb(p['outline_color']))
    pal = list(trk['palette'])
    if p['text_color'] is not None:
        pal[0] = hex_rgb(p['text_color'])
    if p['override_colors']:
        pal = [pal[0]] * 16
    for (x, y), (a, col) in cov.items():
        b = px[x, y]
        c = pal[col]
        px[x, y] = tuple((b[i] * (3 - a) + c[i] * a + 1) // 3 for i in range(3))
    return img, act, (glyphs, cw, ch)


def to_panel(img):
    return img.resize((GRID_W * 2, GRID_H * 2), Image.NEAREST)


def contact_sheet(tracks, names, presets, overrides, video, t_samples, frame, panel=False):
    base = background(video, frame)
    cells = []
    for n in names:
        p = resolve(n, overrides, presets)
        img, act, (glyphs, cw, ch) = render(tracks, p, video, t_samples, base=base)
        ys = [b[1] for b in (event_box(e, glyphs, cw, ch) for e in act) if b]
        ye = [b[3] for b in (event_box(e, glyphs, cw, ch) for e in act) if b]
        if ys:
            y0, y1 = max(0, min(ys) - 40), min(GRID_H, max(ye) + 40)
        else:
            y0, y1 = GRID_H - 240, GRID_H
        cells.append((n, img.crop((0, y0, GRID_W, y1))))
    s = 2 if panel else 1
    ch_ = max(c.size[1] for _, c in cells) * s
    lab = 30 * s
    cols = 2
    rows = (len(cells) + cols - 1) // cols
    gap = 8
    sheet = Image.new('RGB', (cols * GRID_W * s + (cols + 1) * gap, rows * (ch_ + lab) + (rows + 1) * gap), (40, 40, 40))
    font = ImageFont.truetype(os.path.join(HERE, 'fonts', 'Roboto-Regular.ttf'), 18 * s)
    d = ImageDraw.Draw(sheet)
    for i, (n, c) in enumerate(cells):
        x = gap + (i % cols) * (GRID_W * s + gap)
        y = gap + (i // cols) * (ch_ + lab + gap)
        d.text((x + 4, y + 2), n, font=font, fill=(255, 255, 255))
        if s != 1:
            c = c.resize((c.size[0] * s, c.size[1] * s), Image.NEAREST)
        sheet.paste(c, (x, y + lab))
    return sheet


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('input', help='.tnv3 file or subtitle area file')
    ap.add_argument('--video', help='picture size WxH (required for an area file)')
    ap.add_argument('--time', type=float, required=True, help='seconds')
    ap.add_argument('-o', '--out')
    ap.add_argument('--frame', help='image used as the picture (resized to --video if needed); default grey')
    ap.add_argument('--panel', action='store_true', help='1600x1440 nearest-neighbour (2x2 panel pixels per grid pixel)')
    ap.add_argument('--preset')
    ap.add_argument('--sheet', metavar='OUT.png', help='contact sheet with one cell per preset')
    ap.add_argument('--presets', help='comma-separated preset names for --sheet (default: all)')
    ap.add_argument('--track')
    ap.add_argument('--size')
    ap.add_argument('--subs', choices=('on', 'off'))
    ap.add_argument('--hide-sdh', action='store_true', default=None)
    ap.add_argument('--delay', type=int)
    ap.add_argument('--voffset', type=int)
    ap.add_argument('--text-color', '--color0', dest='text_color', metavar='RRGGBB',
                    help='replaces palette entry 0 (ordinary dialogue)')
    ap.add_argument('--override-colors', action='store_true', default=None,
                    help='draw every palette entry in the text colour')
    ap.add_argument('--outline-color', metavar='RRGGBB')
    ap.add_argument('--outline-width', type=int)
    ap.add_argument('--box-color', metavar='RRGGBB')
    ap.add_argument('--box-pad', type=int)
    a = ap.parse_args(argv)
    try:
        video = None
        if a.video:
            video = tuple(int(v) for v in a.video.lower().split('x'))
        tracks, video = load_input(a.input, video)
        ov = {k: getattr(a, k) for k in ('track', 'size', 'subs', 'hide_sdh', 'delay', 'voffset', 'outline_color',
                                         'outline_width', 'box_color', 'box_pad', 'text_color',
                                         'override_colors')}
        t = int(round(a.time * RATE))
        if a.sheet:
            presets = load_presets()
            names = a.presets.split(',') if a.presets else list(presets)
            img = contact_sheet(tracks, names, presets, ov, video, t, a.frame, a.panel)
            img.save(a.sheet)
            return 0
        if not a.out:
            raise RenderError('need -o OUT.png (or --sheet)')
        p = resolve(a.preset, ov)
        img = render(tracks, p, video, t, a.frame)[0]
        if a.panel:
            img = to_panel(img)
        img.save(a.out)
    except (RenderError, OSError, ValueError) as e:
        print('tst_render: %s' % e, file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())

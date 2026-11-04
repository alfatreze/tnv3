#!/usr/bin/env python3
"""TST reference authoring tool: SRT / ASS tracks -> one subtitle area (TNV3 container section 6, draft 1.4).

    tst_build.py --track FILE[:lang[:default][:forced]] [--track ...] --sizes 16x24,20x28,26x36
                 --font [NAME[:STYLE]=]FILE.ttf[@SCALE] [--font ...] [--styles styles.json]
                 --video WxH [--fps N/D] -o AREA.bin
Mux with: tnv3 mux VIDEO.tvc -s AREA.bin -o OUT.tnv3

Fonts: the first --font without STYLE is the default font (family name from NAME, `default` if none). STYLE is one of
regular (default), italic, bold, bolditalic and gives the face used for that family when the text asks for it.
@SCALE multiplies the font size of that face (1.0 = the common size). Every (face, character) pair used becomes its
own glyph; all faces share the cell size and one baseline. See docs/TST_SUBTITLES_V0.md. Needs Pillow only."""
import argparse
import html
import json
import os
import re
import struct
import sys
import unicodedata

from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'tnv3_ref', 'py'))
import tnv3_demux  # noqa: E402
from tst_area import pack_area  # noqa: E402

GRID_W, GRID_H = 800, 720          # the subtitle grid = the output window (container section 6)
BREAK = 0xFFFF                     # line-break reference
SWITCH = 0xFFE0                    # 0xFFE0 + k: switch the current colour to palette entry k
SS = 4                             # supersampling factor for rasterisation
RATE = 48000                       # samples per second
WHITE = (255, 255, 255)
ITALIC_RGB = (255, 224, 64)        # colour used for italics when the family has no italic face
GREY = (160, 160, 160)


class TstError(Exception):
    pass


# --------------------------------------------------------------------------------------------- fonts

class FontSet:
    """Registry of faces and families. A face is (path, scale); a family maps (bold, italic) to a face id."""

    def __init__(self):
        self.faces = []          # (path, scale)
        self.fam = {}            # family (lower case) -> {(bold, italic): face id}
        self.default = None
        self._notdef = {}
        self.log = lambda m: None

    STYLES = {'regular': (False, False), 'italic': (False, True), 'bold': (True, False), 'bolditalic': (True, True)}

    def add(self, spec):
        m = re.fullmatch(r'(?:([^=:]+)(?::([A-Za-z]+))?=)?(.+?)(?:@([0-9.]+))?', spec)
        if not m:
            raise TstError('--font %r: use [NAME[:STYLE]=]FILE[@SCALE]' % spec)
        name, style, path, scale = m.groups()
        name = (name or 'default').strip().lower()
        style = (style or 'regular').lower()
        if style not in self.STYLES:
            raise TstError('--font %r: style must be regular, italic, bold or bolditalic' % spec)
        if not os.path.exists(path):
            raise TstError('font file not found: %s' % path)
        sc = float(scale) if scale else None
        if sc is not None and not 0.3 <= sc <= 3:
            raise TstError('font scale 0.3..3')
        if sc is None:                       # inherit the scale of the family's regular face
            reg = self.fam.get(name, {}).get((False, False))
            sc = self.faces[reg][1] if reg is not None else 1.0
        if self.default is None:
            if style != 'regular':
                raise TstError('the first --font must be a regular face (it is the default font)')
            self.default = name
        key = self.STYLES[style]
        if key in self.fam.setdefault(name, {}):
            raise TstError('--font %s:%s given twice' % (name, style))
        self.fam[name][key] = len(self.faces)
        self.faces.append((path, sc))

    def check(self):
        if not self.faces:
            raise TstError('no --font')
        for n, f in self.fam.items():
            if (False, False) not in f:
                raise TstError('font family %s has no regular face' % n)

    @classmethod
    def make(cls, spec, log=None):
        if isinstance(spec, FontSet):
            return spec
        fs = cls()
        fs.log = log or fs.log
        for s in ([spec] if isinstance(spec, str) else spec):
            fs.add(s)
        fs.check()
        return fs

    def family(self, name, warned):
        """Family for a font name used by the source; unknown names fall back to the default with a warning."""
        k = (name or '').strip().lower()
        if k in self.fam:
            return k
        if k not in warned:
            warned.add(k)
            self.log("warning: font %r not given with --font, using the default font '%s'" % (name, self.default))
        return self.default

    def resolve(self, fam, bold, italic):
        """-> (face id, italic_fallback). Missing bold is ignored; missing italic sets the fallback flag."""
        f = self.fam[fam]
        if (bold, italic) in f:
            return f[(bold, italic)], False
        if italic:
            if (False, True) in f:
                return f[(False, True)], False
            return f.get((bold, False), f[(False, False)]), True
        return f[(False, False)], False

    def has(self, face, c):
        if c == ' ':
            return True
        path = self.faces[face][0]
        if face not in self._notdef:
            self._notdef[face] = MultiFont._img(ImageFont.truetype(path, 40), '͸').tobytes()
        return MultiFont._img(ImageFont.truetype(path, 40), c).tobytes() != self._notdef[face]


class MultiFont:
    """Faces at one common size (face scale applied), one baseline, one cell."""

    def __init__(self, faces, chars, cw, ch, size=None):
        self.faces, self.chars, self.cw, self.ch = faces, chars, cw, ch
        self.fonts = {}
        self.q = None
        if size is None:
            self._fit()
        else:
            self._set(int(round(size * SS)))
        asc = max(f.getmetrics()[0] for f in self.fonts.values())
        desc = max(f.getmetrics()[1] for f in self.fonts.values())
        top = max(0, (ch * SS - (asc + desc)) // 2)
        self.baseline4 = min(ch * SS - SS, (((top + asc) + SS // 2) // SS) * SS)

    @staticmethod
    def _img(font, c):
        im = Image.new('L', (200, 200), 0)
        ImageDraw.Draw(im).text((20, 120), c, font=font, fill=255, anchor='ls')
        return im

    def _set(self, q):
        self.q = q
        self.fonts = {f: ImageFont.truetype(self.faces[f][0], max(4, int(round(q * self.faces[f][1]))))
                      for f in self.chars}

    def advance(self, f, c):
        return max(1, min(self.cw, int(self.fonts[f].getlength(c) / SS + 0.5)))

    def _shift(self, f, c):
        l = self.fonts[f].getbbox(c, anchor='ls')[0]
        return ((-l + SS - 1) // SS) * SS if l < 0 else 0

    def _ink_ok(self, f, c):
        if c == ' ':
            return True
        l, t, r, b = self.fonts[f].getbbox(c, anchor='ls')
        return r + self._shift(f, c) <= self.cw * SS and int(self.fonts[f].getlength(c) / SS + 0.5) <= self.cw

    def _fit(self):
        for q in range(self.ch * SS, 7, -1):
            self._set(q)
            if any(sum(f.getmetrics()) > self.ch * SS for f in self.fonts.values()):
                continue
            if all(self._ink_ok(f, c) for f, cs in self.chars.items() for c in cs):
                return
        raise TstError('no font size fits the %dx%d cell' % (self.cw, self.ch))

    def raster(self, f, c):
        """-> (advance, rows, clipped); rows = cell_h lists of cell_w alpha values 0..3."""
        cw, ch = self.cw, self.ch
        im = Image.new('L', (cw * SS, ch * SS), 0)
        clipped = False
        if c != ' ':
            font = self.fonts[f]
            l, t, r, b = font.getbbox(c, anchor='ls')
            sh = self._shift(f, c)
            if r + sh > cw * SS or self.baseline4 + t < 0 or self.baseline4 + b > ch * SS:
                clipped = True
            ImageDraw.Draw(im).text((sh, self.baseline4), c, font=font, fill=255, anchor='ls')
        small = im.resize((cw, ch), Image.BOX)
        px = small.load()
        rows = [[(px[x, y] * 3 + 127) // 255 for x in range(cw)] for y in range(ch)]
        return self.advance(f, c), rows, clipped


def pack_glyphs(cw, ch, glyphs):
    rowb = (cw + 3) // 4
    out = bytearray(struct.pack('<HBB', len(glyphs), cw, ch))
    for adv, rows in glyphs:
        out.append(adv)
        for row in rows:
            b = bytearray(rowb)
            for x, v in enumerate(row):
                b[x // 4] |= v << (2 * (x % 4))
            out += b
    return bytes(out)


# --------------------------------------------------------------------------------------------- parsing

def _ms(h, m, s, frac):
    return ((int(h) * 60 + int(m)) * 60 + int(s)) * 1000 + int(frac.ljust(3, '0')[:3])


SRT_TIME = re.compile(r'(\d+):(\d+):(\d+)[,.](\d+)\s*-->\s*(\d+):(\d+):(\d+)[,.](\d+)')


def parse_srt(text):
    """-> list of (start_ms, end_ms, markup) in file order."""
    out = []
    lines = text.replace('\r\n', '\n').replace('\r', '\n').split('\n')
    i = 0
    while i < len(lines):
        m = SRT_TIME.search(lines[i])
        if not m:
            i += 1
            continue
        g = m.groups()
        st, en = _ms(*g[0:4]), _ms(*g[4:8])
        i += 1
        body = []
        while i < len(lines) and lines[i].strip() != '':
            body.append(lines[i])
            i += 1
        out.append((st, en, '\n'.join(body)))
    return out


def ass_rgb(s):
    """ASS colour &H[AA]BBGGRR[&] -> (r, g, b)."""
    m = re.search(r'&H([0-9A-Fa-f]+)', s.strip())
    if not m:
        return None
    v = int(m.group(1), 16)
    return (v & 255, (v >> 8) & 255, (v >> 16) & 255)


def parse_ass(text):
    """-> (events, styles); events = (start_ms, end_ms, text, style_name); styles = name -> dict(font, rgb, bold, italic)."""
    text = text.replace('\r\n', '\n').replace('\r', '\n')
    section = ''
    fields = sfields = None
    out, styles = [], {}
    for line in text.split('\n'):
        s = line.strip()
        if s.startswith('['):
            section = s.lower()
            continue
        low = s.lower()
        if section.startswith('[v4') and 'styles' in section:
            if low.startswith('format:'):
                sfields = [f.strip().lower() for f in s[7:].split(',')]
            elif low.startswith('style:') and sfields:
                parts = s[6:].lstrip().split(',', len(sfields) - 1)
                d = dict(zip(sfields, parts))
                styles[d.get('name', '').strip()] = dict(
                    font=d.get('fontname', '').strip(),
                    rgb=ass_rgb(d.get('primarycolour', '')) or WHITE,
                    bold=d.get('bold', '0').strip() not in ('0', ''),
                    italic=d.get('italic', '0').strip() not in ('0', ''))
            continue
        if section != '[events]':
            continue
        if low.startswith('format:'):
            fields = [f.strip().lower() for f in s[7:].split(',')]
        elif low.startswith('dialogue:'):
            if not fields:
                raise TstError('ASS Dialogue before Format')
            parts = s[9:].lstrip().split(',', len(fields) - 1)
            if len(parts) != len(fields):
                raise TstError('ASS Dialogue has too few fields: %r' % s)
            d = dict(zip(fields, parts))
            t = []
            for key in ('start', 'end'):
                m = re.fullmatch(r'\s*(\d+):(\d+):(\d+)[.](\d+)\s*', d[key])
                if not m:
                    raise TstError('ASS time %r' % d[key])
                t.append(_ms(m.group(1), m.group(2), m.group(3), m.group(4)))
            out.append((t[0], t[1], d['text'], d.get('style', 'Default').strip()))
    return out, styles


TAG = re.compile(r'</?[A-Za-z][^<>]*>|\{[^{}]*\}')
HTML_COLORS = {'white': WHITE, 'black': (0, 0, 0), 'red': (255, 0, 0), 'green': (0, 128, 0), 'lime': (0, 255, 0),
               'blue': (0, 0, 255), 'yellow': (255, 255, 0), 'cyan': (0, 255, 255), 'aqua': (0, 255, 255),
               'magenta': (255, 0, 255), 'orange': (255, 165, 0), 'gray': (128, 128, 128), 'grey': (128, 128, 128)}


def html_rgb(s):
    s = s.strip().lower()
    if s in HTML_COLORS:
        return HTML_COLORS[s]
    m = re.fullmatch(r'#?([0-9a-f]{6})', s)
    if m:
        v = int(m.group(1), 16)
        return (v >> 16, (v >> 8) & 255, v & 255)
    return None


def parse_markup(s, ass, base, fam_for, styles=None):
    """-> list of (text, family, bold, italic, rgb). base = dict(fam, bold, italic, rgb) (rgb None = default colour).
    Handles <i> <b> <font color face> (SRT) and {\\i \\b \\fn \\c \\1c \\r \\p} (ASS override blocks, also accepted in SRT);
    every other tag is dropped, ASS drawings (\\p1) are dropped, positioning is ignored."""
    segs = []
    st = dict(base)
    fstack = []
    drawing = False
    pos = 0

    def add(t):
        if drawing or not t:
            return
        if ass:
            t = t.replace('\\N', '\n').replace('\\n', ' ').replace('\\h', ' ')
        else:
            t = html.unescape(t)
        segs.append((t, st['fam'], st['bold'], st['italic'], st['rgb']))

    for m in TAG.finditer(s):
        add(s[pos:m.start()])
        pos = m.end()
        tag = m.group()
        if tag[0] == '{':
            for t in re.findall(r'\\([^\\]*)', tag[1:-1]):
                if re.fullmatch(r'p\d+', t):
                    drawing = int(t[1:]) != 0
                elif t.startswith('fn'):
                    st['fam'] = fam_for(t[2:]) if t[2:] else base['fam']
                elif re.fullmatch(r'1?c(&H[0-9A-Fa-f]+&?)?', t):
                    st['rgb'] = ass_rgb(t) if '&' in t else base['rgb']
                elif re.fullmatch(r'i[01]?', t):
                    st['italic'] = t == 'i1' if len(t) > 1 else base['italic']
                elif re.fullmatch(r'b\d*', t):
                    st['bold'] = int(t[1:]) >= 1 if len(t) > 1 else base['bold']
                elif t == 'r' or (t.startswith('r') and styles and t[1:] in styles):
                    b = base
                    if t != 'r':
                        b = styles[t[1:]]
                    st = dict(b)
        else:
            name = re.match(r'</?([A-Za-z]+)', tag).group(1).lower()
            closing = tag.startswith('</')
            if name == 'i':
                st['italic'] = not closing
            elif name == 'b':
                st['bold'] = not closing
            elif name == 'font':
                if closing:
                    if fstack:
                        st['fam'], st['rgb'] = fstack.pop()
                else:
                    fstack.append((st['fam'], st['rgb']))
                    for am in re.finditer(r'(color|face)\s*=\s*(?:"([^"]*)"|\'([^\']*)\'|([^\s>]+))', tag, re.I):
                        v = am.group(2) or am.group(3) or am.group(4)
                        if am.group(1).lower() == 'color':
                            c = html_rgb(v)
                            if c:
                                st['rgb'] = c
                        else:
                            st['fam'] = fam_for(v)
    add(s[pos:])
    return segs


def to_lines(segs):
    """-> list of lines, each a list of (char, (family, bold, italic), rgb); whitespace collapsed, lines trimmed,
    empty lines dropped."""
    lines = [[]]
    for text, fam, bold, italic, rgb in segs:
        t = unicodedata.normalize('NFC', text)
        for ch in t:
            if ch == '\n':
                lines.append([])
                continue
            cat = unicodedata.category(ch)
            if ch in '\t\r ' or cat == 'Zs':
                ch = ' '
            elif cat in ('Cc', 'Cf', 'Cs', 'Co', 'Cn', 'Zl', 'Zp'):
                continue
            lines[-1].append((ch, (fam, bold, italic), rgb))
    res = []
    for ln in lines:
        out = []
        for it in ln:
            if it[0] == ' ' and (not out or out[-1][0] == ' '):
                continue
            out.append(it)
        while out and out[-1][0] == ' ':
            out.pop()
        if out:
            res.append(out)
    return res


# --------------------------------------------------------------------------------------------- tracks

SDH_GROUP = r'(?:\[[^\[\]]*\]|\([^()]*\))'
SDH_RE = re.compile(r'^(?:-\s*)?%s(?:\s*(?:-\s*)?%s)*$' % (SDH_GROUP, SDH_GROUP))


def is_sdh_text(s):
    """A line that is entirely a bracketed sound description ([...] or (...)) or only music notes."""
    s = s.strip()
    if not s:
        return False
    if all(c in '♪♫ ' for c in s):
        return True
    return bool(SDH_RE.match(s))


def load_styles(path):
    """Speaker style sheet: {"DEMON:": {"font": "Demon", "color": "FF2020", "bold": false, "italic": false}, ...}"""
    if not path:
        return {}
    with open(path, encoding='utf-8') as f:
        d = json.load(f)
    for k, v in d.items():
        if not k or not isinstance(v, dict) or set(v) - {'font', 'color', 'bold', 'italic'}:
            raise TstError('styles.json: entry %r needs an object with font/color/bold/italic' % k)
    return d


def apply_speaker(lines, sheet, fonts, warned):
    """Remove a speaker prefix at the start of a source line (after an optional leading dash) and restyle the rest
    of that line with the sheet's font and colour."""
    out = []
    for ln in lines:
        text = ''.join(it[0] for it in ln)
        off = 2 if text.startswith('- ') else 1 if text.startswith('-') else 0
        for prefix, sty in sheet.items():
            if text[off:].startswith(prefix):
                rest = ln[off + len(prefix):]
                if rest and rest[0][0] == ' ':
                    rest = rest[1:]
                fam = fonts.family(sty['font'], warned) if 'font' in sty else None
                rgb = html_rgb(sty['color']) if 'color' in sty else None
                new = []
                for ch, (f, b, i), c in rest:
                    new.append((ch, (fam or f, sty.get('bold', b), sty.get('italic', i)), rgb if rgb and c is None else c))
                ln = ln[:off] + new
                break
        if ln:
            out.append(ln)
    return out


def load_track(path, fonts, sheet=None, italic_rgb=ITALIC_RGB, log=None):
    """-> dict(events, dropped, n_in, palette). events = [(start_ms, end_ms, [(line, sdh)])] with line =
    [(char, face id, palette index)]; the palette (16 RGB tuples) is built from the colours used, entry 0 being the
    default style's colour."""
    log = log or (lambda m: None)
    with open(path, 'rb') as f:
        text = f.read().decode('utf-8-sig')
    ass = path.lower().endswith(('.ass', '.ssa'))
    warned = set()
    sheet = sheet or {}

    def fam_for(name):
        return fonts.family(name, warned)
    dflt = dict(fam=fonts.default, bold=False, italic=False, rgb=None)
    if ass:
        raw, styles = parse_ass(text)
        sb = {n: dict(fam=fam_for(s['font']), bold=s['bold'], italic=s['italic'], rgb=s['rgb'])
              for n, s in styles.items()}
        first = sb.get('Default') or (next(iter(sb.values())) if sb else dflt)
        default_rgb = first['rgb'] or WHITE
    else:
        raw, sb, first, default_rgb = [(a, b, c, None) for a, b, c in parse_srt(text)], {}, dflt, WHITE
    n_in = len(raw)
    pal = [default_rgb]
    warned_pal = set()
    events, dropped = [], 0
    badchar = set()

    def pal_index(rgb):
        if rgb == default_rgb:
            return 0
        if rgb in pal:
            return pal.index(rgb)
        if len(pal) < 16:
            pal.append(rgb)
            return len(pal) - 1
        k = min(range(len(pal)), key=lambda j: sum((pal[j][c] - rgb[c]) ** 2 for c in range(3)))
        if rgb not in warned_pal:
            warned_pal.add(rgb)
            log('warning: more than 16 colours; %02X%02X%02X mapped to the nearest entry %d' % (rgb + (k,)))
        return k

    for st, en, markup, sname in raw:
        base = sb.get(sname) or first
        lines = to_lines(parse_markup(markup, ass, base, fam_for, sb))
        if sheet:
            lines = apply_speaker(lines, sheet, fonts, warned)
        if not lines:
            dropped += 1
            continue
        joined = ' '.join(''.join(it[0] for it in ln) for ln in lines)
        whole = is_sdh_text(joined)
        out = []
        for ln in lines:
            items = []
            for ch, (fam, bold, ital), rgb in ln:
                face, fb = fonts.resolve(fam, bold, ital)
                rgb = rgb or default_rgb
                if fb and rgb == default_rgb:
                    rgb = italic_rgb
                if not fonts.has(face, ch):
                    if (face, ch) not in badchar:
                        badchar.add((face, ch))
                        log('warning: U+%04X not in font %s, replaced by ?' % (ord(ch), os.path.basename(fonts.faces[face][0])))
                    ch = '?'
                items.append((ch, face, pal_index(rgb)))
            out.append((items, whole or is_sdh_text(''.join(it[0] for it in ln))))
        events.append((st, en, out))
    if not events:
        raise TstError('%s: no visible events' % path)
    palette = pal + [GREY] * (16 - len(pal))
    return dict(events=events, dropped=dropped, n_in=n_in, palette=palette, used_colours=len(pal))


# --------------------------------------------------------------------------------------------- validation

def validate_blocks(sub, glyph):
    """Section 6.2 rules for one event list + glyph table. Returns (event_count, glyph_count); raises TstError."""
    if len(glyph) < 4:
        raise TstError('glyph table shorter than header')
    gc, cw, ch = struct.unpack('<HBB', glyph[:4])
    if not 1 <= gc <= 0xFFE0 or cw < 1 or ch < 1:
        raise TstError('glyph table header')
    rowb = (cw + 3) // 4
    if len(glyph) != 4 + gc * (1 + ch * rowb):
        raise TstError('glyph table length')
    p = 4
    for g in range(gc):
        if not 1 <= glyph[p] <= cw:
            raise TstError('glyph %d advance' % g)
        p += 1
        for _ in range(ch):
            if cw % 4 and glyph[p + rowb - 1] >> (2 * (cw % 4)):
                raise TstError('glyph %d padding bits' % g)
            p += rowb
    if len(sub) < 4:
        raise TstError('subtitle block shorter than count')
    count = struct.unpack('<I', sub[:4])[0]
    p = 4
    prev = 0
    for e in range(count):
        if p + 16 > len(sub):
            raise TstError('event %d beyond block' % e)
        st, en, x, y, col, fl, n = struct.unpack('<IIHHBBH', sub[p:p + 16])
        p += 16
        if not st < en:
            raise TstError('event %d start >= end' % e)
        if x >= GRID_W or y >= GRID_H:
            raise TstError('event %d position' % e)
        if col > 15:
            raise TstError('event %d colour' % e)
        if fl > 3:
            raise TstError('event %d flags' % e)
        if st < prev:
            raise TstError('event %d unsorted' % e)
        prev = st
        if p + 2 * n > len(sub):
            raise TstError('event %d glyph refs beyond block' % e)
        for i in range(n):
            gid = struct.unpack('<H', sub[p + 2 * i:p + 2 * i + 2])[0]
            if gid == BREAK or SWITCH <= gid <= SWITCH + 15:
                continue
            if gid >= gc or gid >= SWITCH:
                raise TstError('event %d glyph id %d out of range' % (e, gid))
        p += 2 * n
    if p != len(sub):
        raise TstError('subtitle block length')
    return count, gc


# --------------------------------------------------------------------------------------------- layout

def wrap_line(line, adv, max_w):
    """Greedy word wrap of one line [(char, face, idx)] with adv(item) = advance; the original space item between
    two words is kept (it stays in the glyph set). Returns a list of lines."""
    words, seps, gap = [[]], [], None
    for it in line:
        if it[0] == ' ':
            if gap is None:
                gap = it
        else:
            if gap is not None:
                if words[-1]:
                    seps.append(gap)
                    words.append([])
                gap = None
            words[-1].append(it)
    words = [w for w in words if w]
    pieces = []                         # (piece, separator item before it or None)
    for wi, w in enumerate(words):
        sep = seps[wi - 1] if wi else None
        cur, wd = [], 0
        for it in w:
            a = adv(it)
            if cur and wd + a > max_w:
                pieces.append((cur, sep))
                sep = None
                cur, wd = [], 0
            cur.append(it)
            wd += a
        pieces.append((cur, sep))
    lines, cur, wd = [], [], 0
    for piece, sep in pieces:
        ww = sum(adv(i) for i in piece)
        sw = adv(sep) if (cur and sep) else 0
        if cur and wd + sw + ww > max_w:
            lines.append(cur)
            cur, wd, sw = [], 0, 0
        if cur and sep:
            cur = cur + [sep] + piece
        else:
            cur = cur + piece
        wd += sw + ww
    if cur:
        lines.append(cur)
    return lines


def snap(ms, fps):
    if not fps:
        return ms * 48
    fn, fd = fps
    k = (2 * ms * fn + 1000 * fd) // (2000 * fd)      # nearest picture index
    return (k * RATE * fd) // fn


def line_refs(items, gid, cur):
    """Glyph references of one line with colour switches; cur = the current colour index. -> (refs, cur)."""
    ids = []
    for ch, face, idx in items:
        if ch != ' ' and idx != cur:
            ids.append(SWITCH + idx)
            cur = idx
        ids.append(gid[(face, ch)])
    return ids, cur


def build_variant(track, fonts, cell, video, fps=None, size=None, margin=40, bottom=24, align='center',
                  placement='auto', forced=False, sdh=True, log=None):
    """One loaded track at one cell size -> (event_list_bytes, glyph_table_bytes, info). Layout depends on the size."""
    log = log or (lambda m: None)
    events = track['events']
    cw, ch = cell
    vw, vh = video
    if not (1 <= cw <= 255 and 1 <= ch <= 255):
        raise TstError('cell size 1..255')
    if not (1 <= vw <= GRID_W and 1 <= vh <= GRID_H):
        raise TstError('video size must fit the 800x720 grid')

    used = sorted({(it[1], it[0]) for _, _, ls in events for ln, _ in ls for it in ln})
    chars = {}
    for f, c in used:
        chars.setdefault(f, []).append(c)
    mf = MultiFont(fonts.faces, chars, cw, ch, size=size)
    gid = {k: i for i, k in enumerate(used)}
    adv = {k: mf.advance(*k) for k in used}
    glyphs, clipped = [], 0
    for f, c in used:
        a, rows, clip = mf.raster(f, c)
        clipped += clip
        glyphs.append((a, rows))
    if clipped:
        log('warning: %d glyphs clipped by the cell' % clipped)
    if len(glyphs) > SWITCH:
        raise TstError('too many glyphs')
    gbytes = pack_glyphs(cw, ch, glyphs)
    wadv = lambda it: adv[(it[1], it[0])]       # noqa: E731

    max_w = GRID_W - 2 * margin
    wrapped = []
    for st, en, ls in events:
        out = []
        for ln, s in ls:
            out += [(w, s) for w in wrap_line(ln, wadv, max_w)]
        wrapped.append((st, en, out))
    maxlines = max(len(o) for _, _, o in wrapped)
    # tallest stack of simultaneously active events (they are stacked upwards, layout rule 5)
    maxstack = max(sum(len(o2) for s2, e2, o2 in wrapped if s2 <= s1 < e2) for s1, _, _ in wrapped)

    pic_top = (GRID_H - vh) // 2
    pic_bot = pic_top + vh
    bar_h = GRID_H - pic_bot
    need = maxstack * ch + bottom + 8
    if placement == 'bar' or (placement == 'auto' and need <= bar_h):
        region, base_bottom = 'bar', GRID_H - bottom
    else:
        region, base_bottom = 'picture', pic_bot - bottom
    if region == 'picture' and base_bottom - maxstack * ch < 0:
        raise TstError('text block does not fit above the picture bottom')

    order = sorted(range(len(wrapped)), key=lambda i: (wrapped[i][0], i))
    active = []
    recs = []            # (start, end, x, y, colour, flags, [refs])
    fbit = 1 if forced else 0
    for i in order:
        st_ms, en_ms, lines = wrapped[i]
        s0, s1 = snap(st_ms, fps), snap(en_ms, fps)
        if s1 <= s0:
            s1 = s0 + (snap(1, None) if not fps else (RATE * fps[1] + fps[0] - 1) // fps[0])
        if s1 > 0xFFFFFFFF:
            raise TstError('time exceeds 32 bits')
        h = len(lines) * ch
        active = [a for a in active if a[0] > s0]
        bot = base_bottom
        moved = True
        while moved:
            moved = False
            for _, yt, yb in active:
                if yt < bot and bot - h < yb:
                    bot = yt
                    moved = True
        ytop = bot - h
        if ytop < 0:
            log('warning: event at %d ms does not fit above the stack; clamped' % st_ms)
            ytop = 0
        active.append((s1, ytop, ytop + h))
        widths = [sum(wadv(it) for it in ln) for ln, _ in lines]
        if align == 'block' and len(lines) > 1 and len({s for _, s in lines}) == 1:
            ids, cur = [], lines[0][0][0][2]
            for j, (ln, _) in enumerate(lines):
                if j:
                    ids.append(BREAK)
                r, cur = line_refs(ln, gid, cur)
                ids += r
            fl = fbit | (2 if sdh and lines[0][1] else 0)
            recs.append((s0, s1, (GRID_W - max(widths)) // 2, ytop, lines[0][0][0][2], fl, ids))
            continue
        for j, (ln, s) in enumerate(lines):
            fl = fbit | (2 if sdh and s else 0)
            ids, _ = line_refs(ln, gid, ln[0][2])       # one event per line; colour switches inside it
            recs.append((s0, s1, (GRID_W - widths[j]) // 2, ytop + j * ch, ln[0][2], fl, ids))
    recs.sort(key=lambda r: r[0])
    sub = struct.pack('<I', len(recs))
    for s0, s1, x, y, col, fl, ids in recs:
        if not (0 <= x < GRID_W and 0 <= y < GRID_H):
            raise TstError('position out of grid: %d,%d' % (x, y))
        sub += struct.pack('<IIHHBBH', s0, s1, x, y, col, fl, len(ids)) + b''.join(struct.pack('<H', i) for i in ids)
    validate_blocks(sub, gbytes)
    info = dict(events_out=len(recs), glyphs=len(glyphs), glyph_bytes=len(gbytes), sub_bytes=len(sub),
                region=region, size=mf.q / SS, maxlines=maxlines, clipped=clipped, cell=(cw, ch),
                refs=sum(len(r[6]) for r in recs), sdh_events=sum(1 for r in recs if r[5] & 2),
                forced_events=sum(1 for r in recs if r[5] & 1),
                switches=sum(1 for r in recs for g in r[6] if SWITCH <= g <= SWITCH + 15), faces=len(chars))
    return sub, gbytes, info


def build_area(tracks, fonts, cells, video, fps=None, size=None, margin=40, bottom=24, align='center',
               placement='auto', sdh=True, styles=None, italic_rgb=ITALIC_RGB, log=None):
    """tracks: list of dict(path, lang='und', default=False, forced=False); fonts: FontSet, font spec list or a single
    font file; cells: list of (cw, ch) with increasing height; styles: speaker style sheet (dict).
    Returns (area_bytes, info) with info['tracks'][t]['variants'][v] = variant info."""
    fonts = FontSet.make(fonts, log)
    if log:
        fonts.log = log
    if not tracks:
        raise TstError('no tracks')
    if not cells:
        raise TstError('no sizes')
    for a, b in zip(cells, cells[1:]):
        if b[1] <= a[1]:
            raise TstError('sizes must have strictly increasing height')
    if size is not None and len(cells) > 1:
        raise TstError('--font-size needs a single size')
    out, tinfo = [], []
    for t in tracks:
        trk = load_track(t['path'], fonts, styles, italic_rgb, log)
        vs, vinfo = [], []
        for cell in cells:
            ev, gl, inf = build_variant(trk, fonts, cell, video, fps, size, margin, bottom, align, placement,
                                        forced=t.get('forced', False), sdh=sdh, log=log)
            vs.append((ev, gl))
            vinfo.append(inf)
        out.append(dict(lang=t.get('lang', 'und'), default=t.get('default', False),
                        forced_track=t.get('forced', False), palette=trk['palette'], variants=vs))
        tinfo.append(dict(path=t['path'], events_in=trk['n_in'], dropped=trk['dropped'], variants=vinfo,
                          palette=trk['palette'], colours=trk['used_colours']))
    try:
        area = pack_area(out)
        tnv3_demux.check_area(area)
    except (tnv3_demux.TnvError, ValueError) as e:
        raise TstError('area failed the container check: %s' % e)
    return area, dict(tracks=tinfo, bytes=len(area))


def parse_wh(s, what):
    m = re.fullmatch(r'(\d+)[xX](\d+)', s.strip())
    if not m:
        raise TstError('%s must be WxH' % what)
    return int(m.group(1)), int(m.group(2))


def parse_track_spec(spec):
    parts = spec.split(':')
    t = dict(path=parts[0], lang='und', default=False, forced=False)
    rest = parts[1:]
    if rest and rest[0] not in ('default', 'forced') and rest[0]:
        t['lang'] = rest.pop(0)
    elif rest and not rest[0]:
        rest.pop(0)
    for r in rest:
        if r == 'default':
            t['default'] = True
        elif r == 'forced':
            t['forced'] = True
        else:
            raise TstError('track option %r (use default or forced)' % r)
    return t


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('-o', '--out', required=True, help='subtitle area file')
    ap.add_argument('--track', action='append', required=True, metavar='FILE[:lang[:default][:forced]]',
                    help='SRT or ASS track; repeat for several tracks')
    ap.add_argument('--sizes', default='20x28', help='cell sizes WxH, increasing height, e.g. 16x24,20x28,26x36')
    ap.add_argument('--font', action='append', required=True, metavar='[NAME[:STYLE]=]FILE[@SCALE]',
                    help='font face; repeat for several fonts/styles; the first plain one is the default font')
    ap.add_argument('--styles', metavar='styles.json', help='speaker prefix -> font and colour')
    ap.add_argument('--italic-color', default='FFE040', metavar='RRGGBB',
                    help='colour for italics when the family has no italic face')
    ap.add_argument('--video', required=True, help='picture size WxH (centred vertically in 800x720)')
    ap.add_argument('--fps', help='N/D: snap event times to picture boundaries')
    ap.add_argument('--font-size', type=float, help='base font size in grid pixels (single size only; default: largest that fits)')
    ap.add_argument('--margin', type=int, default=40, help='left/right margin of the wrap width')
    ap.add_argument('--bottom', type=int, default=24, help='distance of the last line from the region bottom')
    ap.add_argument('--align', choices=('center', 'block'), default='center')
    ap.add_argument('--placement', choices=('auto', 'bar', 'picture'), default='auto')
    ap.add_argument('--no-sdh', action='store_true', help='do not set the sdh flag automatically')
    a = ap.parse_args(argv)
    log = lambda m: print(m, file=sys.stderr)       # noqa: E731
    try:
        fps = None
        if a.fps:
            m = re.fullmatch(r'(\d+)/(\d+)', a.fps)
            if not m:
                raise TstError('--fps must be N/D')
            fps = (int(m.group(1)), int(m.group(2)))
        cells = [parse_wh(s, '--sizes') for s in a.sizes.split(',')]
        tracks = [parse_track_spec(s) for s in a.track]
        ic = html_rgb(a.italic_color)
        if not ic:
            raise TstError('--italic-color RRGGBB')
        area, info = build_area(tracks, FontSet.make(a.font, log), cells, parse_wh(a.video, '--video'), fps,
                                a.font_size, a.margin, a.bottom, a.align, a.placement, sdh=not a.no_sdh,
                                styles=load_styles(a.styles), italic_rgb=ic, log=log)
    except (TstError, OSError, json.JSONDecodeError) as e:
        print('tst_build: %s' % e, file=sys.stderr)
        return 2
    with open(a.out, 'wb') as f:
        f.write(area)
    for t in info['tracks']:
        for v in t['variants']:
            print('%s %dx%d: %d events (%d dropped), %d records (%d sdh, %d forced, %d colour switches), glyphs %d (%d B, '
                  '%d faces), events %d B, font %.2f px, %s, max lines %d, %d colours' % (
                      os.path.basename(t['path']), v['cell'][0], v['cell'][1], t['events_in'], t['dropped'],
                      v['events_out'], v['sdh_events'], v['forced_events'], v['switches'], v['glyphs'],
                      v['glyph_bytes'], v['faces'], v['sub_bytes'], v['size'], v['region'], v['maxlines'],
                      t['colours']))
    print('area %d B' % info['bytes'])
    return 0


if __name__ == '__main__':
    sys.exit(main())

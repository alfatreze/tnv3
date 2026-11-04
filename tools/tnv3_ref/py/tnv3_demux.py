"""Independent TNV3 demuxer/validator, written only from docs/TNV3_CONTAINER_V0.md
(plus TVC sections 1, 2, 8 and TAC sections 1-3)."""
import struct
import sys
import zlib


class TnvError(Exception):
    pass


def bad(msg):
    raise TnvError(msg)


def align4(x):
    return (x + 3) & ~3


def crc32(b):
    return zlib.crc32(b) & 0xFFFFFFFF


LEVELS = {  # maxW, maxH, mb/pic, fps, mb/s, bytes/pic, window bytes, min slices
    1: (400, 240, 375, 30, 11250, 96000, 200000, 1),
    2: (800, 464, 1450, 30, 43500, 256000, 400000, 1),
    3: (800, 720, 2250, 30, 67500, 384000, 600000, 2),
}


def S(k, fn, fd):
    return (k * 48000 * fd) // fn


def check_variant(ev, gl, vcw, vch):
    if len(gl) < 4:
        bad("glyph table too short")
    gc, cw, ch = struct.unpack("<HBB", gl[0:4])
    if gc < 1 or gc > 0xFFE0 or cw < 1 or ch < 1:
        bad("glyph table header")
    if (cw, ch) != (vcw, vch):
        bad("variant cell size differs from its glyph table")
    rowb = (cw + 3) // 4
    if len(gl) != 4 + gc * (1 + ch * rowb):
        bad("glyph table length")
    gp = 4
    for _ in range(gc):
        if not 1 <= gl[gp] <= cw:
            bad("glyph advance")
        gp += 1
        for _ in range(ch):
            if cw % 4 and (gl[gp + rowb - 1] >> (2 * (cw % 4))):
                bad("glyph padding bits")
            gp += rowb
    if len(ev) < 4:
        bad("event list too short")
    count, sp, prev = struct.unpack("<I", ev[0:4])[0], 4, 0
    for _ in range(count):
        if sp + 16 > len(ev):
            bad("event beyond list")
        st, en, x, y, col, fl, nn = struct.unpack("<IIHHBBH", ev[sp:sp + 16])
        sp += 16
        if not st < en or x >= 800 or y >= 720 or col > 15 or fl > 3 or st < prev:
            bad("subtitle event fields")
        prev = st
        if sp + 2 * nn > len(ev):
            bad("glyph refs beyond list")
        for i in range(nn):
            gid = struct.unpack("<H", ev[sp + 2 * i:sp + 2 * i + 2])[0]
            if 0xFFF0 <= gid < 0xFFFF:
                bad("reserved glyph reference code")
            if gid < 0xFFE0 and gid >= gc:
                bad("glyph id out of range")
        sp += 2 * nn
    if sp != len(ev):
        bad("event list length")


def check_area(a):
    if len(a) < 4 or not 1 <= a[0] <= 15 or any(a[1:4]):
        bad("track count / reserved")
    nt, dpos = a[0], 4
    for _ in range(nt):
        if dpos + 56 > len(a) or not 1 <= a[dpos + 5] <= 8:
            bad("directory")
        dpos += 56 + 20 * a[dpos + 5]
    if dpos > len(a):
        bad("directory")
    nxt, pos, ndef = align4(dpos), 4, 0
    if any(a[dpos:nxt]):
        bad("subtitle padding")
    for t in range(nt):
        lang = a[pos:pos + 4].rstrip(b"\0")
        if len(lang) < 2 or b"\0" in lang or not all(97 <= c <= 122 for c in lang):
            bad("track language")
        fl, nv = a[pos + 4], a[pos + 5]
        if fl > 3 or a[pos + 6] or a[pos + 7]:
            bad("track flags / reserved")
        ndef += fl & 1
        prevh = 0
        for v in range(nv):
            cw, ch, rsv, eo, el, go, gl = struct.unpack_from("<BBHIIII", a, pos + 56 + 20 * v)
            if rsv or ch <= prevh:
                bad("variant reserved / order")
            prevh = ch
            if eo != nxt or eo + el > len(a):
                bad("event list offset")
            if go != align4(eo + el) or go + gl > len(a) or any(a[eo + el:go]):
                bad("glyph table offset")
            check_variant(a[eo:eo + el], a[go:go + gl], cw, ch)
            nxt = align4(go + gl)
            if any(a[go + gl:nxt]):
                bad("subtitle padding")
            if t == nt - 1 and v == nv - 1 and go + gl != len(a):
                bad("subtitle area length")
        pos += 56 + 20 * nv
    if ndef > 1:
        bad("more than one default track")


def parse(data):
    data = bytes(data)
    size = len(data)
    if size < 64:
        bad("file shorter than header")
    if data[0:4] != b"TNV3":
        bad("magic")
    (version, flags, width, height, fps_num, fps_den, level, channels, res22,
     rate, frame_count, audio_samples, tac_frames, index_offset, sub_offset,
     sub_length, glyph_offset, glyph_length, hcrc) = struct.unpack(
        "<HHHHIIBBHIIIIIIIIII", data[4:64])
    if crc32(data[0:60]) != hcrc:
        bad("header CRC")
    if version != 0:
        bad("version")
    if flags & ~7:
        bad("reserved flag bits")
    f_crc, f_audio, f_subs = flags & 1, (flags >> 1) & 1, (flags >> 2) & 1
    if not (1 <= width <= 65535 and 1 <= height <= 65535):
        bad("size range")
    if not (1 <= fps_num <= 65535 and 1 <= fps_den <= 65535):
        bad("frame rate range")
    if level not in (1, 2, 3):
        bad("level")
    if res22 != 0:
        bad("reserved bytes 22")
    if f_audio:
        if channels not in (1, 2):
            bad("channels")
        if rate != 48000:
            bad("sample rate")
        if tac_frames != (audio_samples + 1023) // 1024 + 1:
            bad("tac_frames formula")
    else:
        if channels != 0 or rate != 0 or audio_samples != 0 or tac_frames != 0:
            bad("audio fields non-zero without audio")
    if frame_count < 1:
        bad("frame_count")
    if not f_subs:
        if sub_offset or sub_length:
            bad("subtitle fields non-zero without subs")

    # ---- chunks ----
    pos = 64
    chunks = []
    for n in range(frame_count):
        if pos + 16 > size:
            bad("chunk %d header beyond file" % n)
        chunk_len, pic_len, n_audio, cflags, ccrc = struct.unpack("<IIHHI", data[pos:pos + 16])
        if chunk_len < 16 or pos + chunk_len > size:
            bad("chunk %d length" % n)
        if pic_len < 3:
            bad("pic_len < 3")
        if 16 + pic_len > chunk_len:
            bad("picture outside chunk")
        pic = data[pos + 16:pos + 16 + pic_len]
        ptype = pic[0]
        if ptype not in (0, 1, 2):
            bad("picture type")
        if cflags & ~3:
            bad("reserved chunk flag bits")
        if (cflags & 1) != (ptype == 0) or ((cflags >> 1) & 1) != (ptype == 2):
            bad("chunk flags")
        if not f_audio and n_audio:
            bad("n_audio without audio")
        p = pos + 16 + pic_len
        end = pos + chunk_len
        frames = []
        for _ in range(n_audio):
            if p + 2 > end:
                bad("TAC frame outside chunk")
            ln = struct.unpack("<H", data[p:p + 2])[0]
            if ln < 1:
                bad("TAC frame length 0")
            if p + 2 + ln > end:
                bad("TAC frame outside chunk")
            frames.append(data[p + 2:p + 2 + ln])
            p += 2 + ln
        if p != end:
            bad("chunk_len mismatch")
        if f_crc:
            if crc32(data[pos + 16:end]) != ccrc:
                bad("chunk CRC")
        elif ccrc != 0:
            bad("chunk CRC field non-zero")
        chunks.append(dict(offset=pos, pic=pic, ptype=ptype, cflags=cflags,
                           frames=frames, clen=chunk_len))
        nxt = align4(end)
        if nxt > size:
            bad("padding beyond file")
        if any(data[end:nxt]):
            bad("chunk padding non-zero")
        pos = nxt
    if chunks[0]["ptype"] != 0:
        bad("first chunk not key")
    anchors = 0
    for n, c in enumerate(chunks):
        t = c["ptype"]
        if t == 1 and anchors < 1:
            bad("P picture without anchor")
        if t == 2 and anchors < 2:
            bad("B picture without two anchors")
        if t in (0, 1):
            anchors += 1
    if index_offset != pos:
        bad("index_offset")
    for n, c in enumerate(chunks):
        pic = c["pic"]
        if pic[1] > 51:
            bad("picture_qp")
        if pic[2] & ~0xF:
            bad("picture flags reserved")

    # ---- TAC frames ----
    allframes = []
    first_tac = []
    for c in chunks:
        first_tac.append(len(allframes))
        allframes.extend(c["frames"])
    if f_audio and len(allframes) != tac_frames:
        bad("TAC frame total")

    def reset_bit(b):
        bb = bytes(b[1:5]).ljust(4, b"\0")
        return 1 if int.from_bytes(bb, "big") >= 0x7FFFFFFF else 0
    if f_audio and not reset_bit(allframes[0]):
        bad("TAC frame 0 reset")

    # ---- display order, timing ----
    disp = [0] * frame_count
    out = 0
    held = None
    for n, c in enumerate(chunks):
        if c["ptype"] == 2:
            disp[n] = out
            out += 1
        else:
            if held is not None:
                disp[held] = out
                out += 1
            held = n
    if held is not None:
        disp[held] = out
        out += 1
    if sorted(disp) != list(range(frame_count)):
        bad("display indices not a permutation")
    pts = [S(disp[n], fps_num, fps_den) for n in range(frame_count)]
    if any(v > 0xFFFFFFFF for v in pts):
        bad("S(disp) exceeds 32 bits")
    if f_audio:
        D = -1
        F = 0
        for n in range(frame_count):
            D = max(D, disp[n])
            F += len(chunks[n]["frames"])
            need = min(tac_frames, S(D + 1, fps_num, fps_den) // 1024 + 2)
            if F < need:
                bad("audio interleaving: lower bound at chunk %d" % n)
            if n < frame_count - 1 and F > need + 4:
                bad("audio interleaving: upper bound at chunk %d" % n)

    # ---- index ----
    ipos = index_offset
    if ipos + 16 * frame_count > size:
        bad("index beyond file")
    for n in range(frame_count):
        off, s, ft, w3 = struct.unpack("<IIII", data[ipos + 16 * n:ipos + 16 * n + 16])
        if off != chunks[n]["offset"]:
            bad("index entry %d offset" % n)
        if s != pts[n]:
            bad("index entry %d pts" % n)
        if ft != first_tac[n]:
            bad("index entry %d first_tac" % n)
        seek = 0
        if chunks[n]["ptype"] == 0 and f_audio:
            seek = max(r for r in range(tac_frames)
                       if r * 1024 <= pts[n] and reset_bit(allframes[r]))
        if w3 != (chunks[n]["cflags"] | (seek << 8)):
            bad("index entry %d flags/seek_tac" % n)
    end_idx = index_offset + 16 * frame_count

    # ---- level limits ----
    maxw, maxh, maxmb, maxfps, maxmbs, maxbpp, maxwin, minsl = LEVELS[level]
    W = (width + 15) & ~15
    H = (height + 15) & ~15
    mbw, mbh = W // 16, H // 16
    if W > maxw or H > maxh:
        bad("level: picture size")
    if mbw * mbh > maxmb:
        bad("level: macroblocks per picture")
    if fps_num > maxfps * fps_den:
        bad("level: frame rate")
    if mbw * mbh * fps_num > maxmbs * fps_den:
        bad("level: macroblocks per second")
    for n, c in enumerate(chunks):
        pic = c["pic"]
        if len(pic) > maxbpp:
            bad("level: bytes per picture")
        if pic[2] & 8:
            rows = pic[3]
            if not 1 <= rows <= 255:
                bad("slice rows")
            ns = (mbh + rows - 1) // rows
            if len(pic) < 4 + 4 * (ns - 1):
                bad("slice table beyond picture")
            lens = struct.unpack("<%dI" % (ns - 1), pic[4:4 + 4 * (ns - 1)])
            if 4 + 4 * (ns - 1) + sum(lens) > len(pic):
                bad("slice lengths exceed picture")
        else:
            ns = 1
        if ns < minsl:
            bad("level: slices per picture")
    w = -(-fps_num // fps_den)
    lens_ = [len(c["pic"]) for c in chunks]
    for i in range(len(lens_)):
        if sum(lens_[max(0, i - w + 1):i + 1]) > maxwin:
            bad("level: bytes per window")
    # section 4.2 (draft 1.2, added by the spec author, not part of the clean-room review)
    clens = [c["clen"] for c in chunks]
    for i in range(len(clens)):
        if sum(clens[max(0, i - w + 1):i + 1]) > maxwin + 65536:
            bad("chunk bytes per window")

    # ---- subtitles (draft 1.3 subtitle area; updated by the spec author, not part of the clean-room review) ----
    subs = None
    if glyph_offset or glyph_length:
        bad("reserved header bytes 52..59")
    if f_subs:
        if sub_offset != end_idx or sub_offset + sub_length > size:
            bad("sub_offset")
        if size != sub_offset + sub_length:
            bad("file size")
        subs = data[sub_offset:sub_offset + sub_length]
        check_area(subs)
    else:
        if size != end_idx:
            bad("file size")

    return dict(width=width, height=height, fps_num=fps_num, fps_den=fps_den,
                level=level, channels=channels, audio=bool(f_audio),
                audio_samples=audio_samples, tac_frames=tac_frames,
                pictures=[c["pic"] for c in chunks], tac=allframes,
                subs=subs)


def demux(path):
    with open(path, "rb") as f:
        info = parse(f.read())
    tvc = b"TVC1" + struct.pack("<HHHHIB3x", info["width"], info["height"],
                                info["fps_num"], info["fps_den"],
                                len(info["pictures"]), info["level"])
    for p in info["pictures"]:
        tvc += struct.pack("<I", len(p)) + p
    tac = None
    if info["audio"]:
        tac = b"TAC0" + struct.pack("<IB3xII", 48000, info["channels"],
                                    info["tac_frames"], info["audio_samples"])
        for fr in info["tac"]:
            tac += struct.pack("<H", len(fr)) + fr
    return tvc, tac, info["subs"]


if __name__ == "__main__":
    try:
        t, a, s, g = demux(sys.argv[1])
    except TnvError as e:
        print("invalid:", e)
        sys.exit(3)
    print("ok", len(t), None if a is None else len(a))

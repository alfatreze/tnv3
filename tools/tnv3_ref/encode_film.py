#!/usr/bin/env python3
"""Encode a whole film into a TNV3 file with the reference tools (TVC video, TAC audio, TNV3 container) and report size,
rates and (optionally) quality.

    python3 tools/tnv3_ref/encode_film.py SOURCE OUT.tnv3 [--qp 30] [--width 800|native] [--fps auto|N/D]
        [--seg 480] [--jobs 12] [--cpu-budget N] [--encoder-threads 1|2]
        [--audio-kbps 96] [--seconds S] [--quality] [--work DIR] [--keep]

Video: ffmpeg decodes and scales (lanczos, 4:2:0) once; the frames are cut into segments of SEG frames that tvc_enc
encodes in parallel (tvc_enc holds its whole input in memory, and is single-threaded). Each segment starts with an I
picture (the GOP is 96 frames, so a 480-frame segment adds at most one extra I picture per 20 s). The level is the
smallest that fits; tvc_enc enforces its per-picture and per-second byte limits inside a segment, and a segment whose
first second breaks the window rule together with the previous segment's tail is re-encoded with that tail (-P).
Audio: ffmpeg to 48 kHz stereo PCM, tac enc at 96 kbit/s. Mux: tnv3 mux -T -c, then tnv3 check.
Quality (--quality): every segment is decoded with tvc_dec and compared with its source frames by ffmpeg (VMAF 0.6.1,
PSNR-Y, SSIM-Y); the report gives frame-weighted means and the worst segment.
Writes OUT.tnv3 and OUT.json (the report)."""
import argparse, concurrent.futures as cf, json, math, os, re, shutil, struct, subprocess, sys, tempfile, time
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))
TVC_ENC = os.path.join(HERE, '..', 'tvc_ref', 'tvc_enc')
TVC_DEC = os.path.join(HERE, '..', 'tvc_ref', 'tvc_dec')
TAC = os.path.join(HERE, '..', 'tac_ref', 'tac')
TNV3 = os.path.join(HERE, 'tnv3')
LEVELS = {1: (400, 240, 375, 11250, 200000), 2: (800, 464, 1450, 43500, 400000), 3: (800, 720, 2250, 67500, 600000)}
STD_RATES = [Fraction(24000, 1001), Fraction(24), Fraction(25), Fraction(30000, 1001), Fraction(30)]

def encoder_schedule(jobs, cpu_budget, encoder_threads):
    """Bound concurrent TVC encoder slots; ffmpeg/audio/OS threads are separate."""
    if jobs < 1 or encoder_threads not in (1, 2):
        raise ValueError('jobs must be positive and encoder threads must be 1 or 2')
    if cpu_budget is None:
        return jobs
    if cpu_budget < encoder_threads:
        raise ValueError('CPU budget must cover at least one encoder job')
    return min(jobs, cpu_budget // encoder_threads)


def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)


def probe(src):
    out = subprocess.run(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
                          'stream=width,height,r_frame_rate:format=duration', '-of', 'json', src],
                         capture_output=True, text=True, check=True).stdout
    j = json.loads(out)
    s = j['streams'][0]
    return s['width'], s['height'], Fraction(s['r_frame_rate']), float(j['format']['duration'])


def pick_level(w, h, fps):
    W, H = (w + 15) & ~15, (h + 15) & ~15
    mbs = W // 16 * (H // 16)
    for lv, (mw, mh, mmb, mmbps, _) in LEVELS.items():
        if W <= mw and H <= mh and mbs <= mmb and fps <= 30 and mbs * fps <= mmbps:
            return lv
    sys.exit('%dx%d at %s fps fits no level' % (w, h, fps))


def tvc_lengths(path):
    b = open(path, 'rb').read()
    hdr = 20 if b[3:4] == b'1' else 16
    n = struct.unpack_from('<I', b, 12)[0]
    p, lens, pics = hdr, [], []
    for _ in range(n):
        l = struct.unpack_from('<I', b, p)[0]
        lens.append(l); pics.append(b[p + 4:p + 4 + l]); p += 4 + l
    return b[:hdr], lens, pics


def window_ok(lens, win, limit):
    s = 0
    for i, l in enumerate(lens):
        s += l
        if i >= win:
            s -= lens[i - win]
        if s > limit:
            return False
    return True


def quality(seg_src, seg_tvc, tmp):
    dec = seg_tvc[:-4] + '_dec.y4m'
    subprocess.run([TVC_DEC, seg_tvc, dec], check=True, capture_output=True)
    r = subprocess.run(['ffmpeg', '-hide_banner', '-nostats', '-i', dec, '-i', seg_src, '-lavfi',
                        '[0:v]split=3[a][b][s];[1:v]split=3[c][e][t];[a][c]libvmaf=n_threads=1;[b][e]psnr;[s][t]ssim',
                        '-f', 'null', '-'], capture_output=True, text=True)
    os.remove(dec)
    v = re.search(r'VMAF score: ([\d.]+)', r.stderr)
    p = re.search(r'PSNR y:([\d.inf]+)', r.stderr)
    s = re.search(r'SSIM Y:([\d.]+)', r.stderr)
    if not (v and p and s):
        raise RuntimeError('quality measurement failed: ' + r.stderr[-400:])
    return float(v[1]), float(p[1]), float(s[1])


def chunk_rate(path, win):
    """worst sum of chunk_len over ceil(fps) consecutive chunks, and mean bytes per second"""
    b = open(path, 'rb').read()
    nf = struct.unpack_from('<I', b, 28)[0]
    p, cl = 64, []
    for _ in range(nf):
        l = struct.unpack_from('<I', b, p)[0]; cl.append(l); p = (p + l + 3) & ~3
    worst = max(sum(cl[max(0, i - win + 1):i + 1]) for i in range(nf))
    return worst


def main():
    global TVC_ENC
    ap = argparse.ArgumentParser()
    ap.add_argument('src'); ap.add_argument('out')
    ap.add_argument('--qp', type=int, default=30)
    ap.add_argument('--width', default='800')
    ap.add_argument('--fps', default='auto')
    ap.add_argument('--seg', type=int, default=480)
    ap.add_argument('--jobs', type=int, default=max(1, (os.cpu_count() or 2) - 2))
    ap.add_argument('--cpu-budget', type=int, default=None, help='maximum CPU slots reserved for concurrent TVC encoders')
    ap.add_argument('--encoder-threads', type=int, choices=[1, 2], default=None,
                    help='threads per TVC job for an encoder supporting TVC_B_WORKERS; default 1 with --cpu-budget')
    ap.add_argument('--experimental-parallel-slices', action='store_true',
                    help='use the separately built experimental slice encoder')
    ap.add_argument('--audio-kbps', type=int, default=96)
    ap.add_argument('--seconds', type=float, default=0)
    ap.add_argument('--quality', action='store_true')
    ap.add_argument('--work', default='')
    ap.add_argument('--keep', action='store_true')
    o = ap.parse_args()
    if o.experimental_parallel_slices:
        TVC_ENC = os.path.join(HERE, '..', 'tvc_ref', 'tvc_enc_slice_experimental')
        if not os.path.isfile(TVC_ENC):
            ap.error('experimental slice encoder not built; run make experimental')
        if o.encoder_threads is None:
            o.encoder_threads = 2
        if o.cpu_budget is None:
            o.cpu_budget = max(o.encoder_threads, os.cpu_count() or o.encoder_threads)
    try:
        jobs = encoder_schedule(o.jobs, o.cpu_budget, o.encoder_threads or 1)
    except ValueError as exc:
        ap.error(str(exc))
    encoder_env = None
    if o.cpu_budget is not None or o.encoder_threads is not None:
        encoder_env = os.environ.copy(); encoder_env['TVC_B_WORKERS'] = str(o.encoder_threads or 1)
    t0 = time.time()
    sw, sh, sfps, dur = probe(o.src)
    near = min(STD_RATES, key=lambda r: abs(float(sfps / r) - 1))      # snap e.g. 1199/50 to 24000/1001 (within 0.05%)
    fps = Fraction(o.fps) if o.fps != 'auto' else (near if abs(float(sfps / near) - 1) < 5e-4 else sfps)
    if fps.denominator > 65535 or fps.numerator > 65535:
        sys.exit('frame rate %s does not fit the container' % fps)
    w = sw if o.width == 'native' else int(o.width)
    h = sh if o.width == 'native' else max(2, int(round(w * sh / sw / 2)) * 2)
    level = pick_level(w, h, fps)
    win = math.ceil(fps)
    limit = LEVELS[level][4]
    work = o.work or tempfile.mkdtemp(prefix='tnv3_film_')
    os.makedirs(work, exist_ok=True)
    log('source %dx%d %s fps %.1f s -> %dx%d at %s fps, level %d, QP %d, %d jobs, work %s' % (sw, sh, sfps, dur, w, h, fps, level, o.qp, jobs, work))

    # 1. decode once, cut into segments
    vf = 'fps=%s,scale=%d:%d:flags=lanczos,format=yuv420p' % (fps, w, h)
    cmd = ['ffmpeg', '-v', 'error', '-i', o.src] + (['-t', str(o.seconds)] if o.seconds else []) + ['-vf', vf, '-f', 'yuv4mpegpipe', '-']
    ff = subprocess.Popen(cmd, stdout=subprocess.PIPE, bufsize=1 << 22)
    header = ff.stdout.readline()
    fsz = w * h + 2 * ((w + 1) // 2) * ((h + 1) // 2)
    segs, n, fh, seg_frames = [], 0, None, []

    def start_encode(i):
        return pool.submit(subprocess.run, [TVC_ENC, segs[i] + '.y4m', segs[i] + '.tvc', '-q', str(o.qp), '-l', str(level)],
                           capture_output=True, text=True, env=encoder_env)

    pool = cf.ThreadPoolExecutor(jobs)
    futs = []
    while True:
        line = ff.stdout.readline()
        if not line:
            break
        frame = ff.stdout.read(fsz)
        if len(frame) < fsz:
            break
        if n % o.seg == 0:
            if fh:
                fh.close(); futs.append(start_encode(len(segs) - 1))
            segs.append(os.path.join(work, 'seg%05d' % len(segs))); seg_frames.append(0)
            fh = open(segs[-1] + '.y4m', 'wb'); fh.write(header)
        fh.write(line); fh.write(frame); n += 1; seg_frames[-1] += 1
    if fh:
        fh.close(); futs.append(start_encode(len(segs) - 1))
    ff.wait()
    log('%d frames in %d segments; encoding' % (n, len(segs)))

    # audio meanwhile
    secs = n * fps.denominator / fps.numerator
    wav, tac = os.path.join(work, 'audio.wav'), os.path.join(work, 'audio.tac')
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', o.src, '-vn', '-t', '%.6f' % secs, '-ac', '2', '-ar', '48000', '-c:a', 'pcm_s16le', wav], check=True)
    afut = pool.submit(subprocess.run, [TAC, 'enc', wav, tac, '-b', str(o.audio_kbps)], capture_output=True, text=True)
    stats = []
    for i, f in enumerate(futs):
        r = f.result()
        if r.returncode:
            sys.exit('tvc_enc failed on %s: %s' % (segs[i], r.stderr))
        stats.append(r.stderr.strip())
        if (i + 1) % 10 == 0 or i + 1 == len(futs):
            log('encoded %d / %d segments' % (i + 1, len(futs)))
    ar = afut.result()
    if ar.returncode:
        sys.exit('tac enc failed: ' + ar.stderr)
    raised = sum(int(re.search(r'(\d+) pictures re-encoded', s)[1]) for s in stats)
    cuts = sum(int(re.search(r'(\d+) scene-cut', s)[1]) for s in stats)

    # 2. joints: the rate window across segment boundaries
    lens_all, pics_all, reenc = [], [], 0
    for i, sgp in enumerate(segs):
        hdr0, lens, pics = tvc_lengths(sgp + '.tvc')
        if i and not window_ok(lens_all[-(win - 1):] + lens, win, limit):
            prev = ','.join(str(x) for x in lens_all[-(win - 1):])
            subprocess.run([TVC_ENC, sgp + '.y4m', sgp + '.tvc', '-q', str(o.qp), '-l', str(level), '-P', prev], check=True, capture_output=True, env=encoder_env)
            hdr0, lens, pics = tvc_lengths(sgp + '.tvc'); reenc += 1
        if i == 0:
            hdr = bytearray(hdr0)
        lens_all += lens; pics_all += pics
    struct.pack_into('<HH', hdr, 8, fps.numerator, fps.denominator)
    struct.pack_into('<I', hdr, 12, len(pics_all))
    tvc = os.path.join(work, 'video.tvc')
    with open(tvc, 'wb') as f:
        f.write(hdr)
        for p in pics_all:
            f.write(struct.pack('<I', len(p))); f.write(p)
    log('video %d bytes; %d segment(s) re-encoded at a joint; window rule %s' % (os.path.getsize(tvc), reenc, 'ok' if window_ok(lens_all, win, limit) else 'BROKEN'))

    # 3. mux and check
    subprocess.run([TNV3, 'mux', tvc, '-a', tac, '-T', '-c', '-o', o.out], check=True)
    chk = subprocess.run([TNV3, 'check', o.out], capture_output=True, text=True)
    info = subprocess.run([TNV3, 'info', o.out], capture_output=True, text=True).stdout.strip()
    log('mux: %s; %s' % (chk.stdout.strip() or chk.stderr.strip(), info.replace('\n', ' | ')))
    size = os.path.getsize(o.out)
    vbytes = sum(lens_all)
    abytes = os.path.getsize(tac) - 20 - 2 * (struct.unpack_from('<I', open(tac, 'rb').read(20), 12)[0])
    rep = dict(source=o.src, width=w, height=h, fps=str(fps), level=level, qp=o.qp, frames=n, seconds=round(secs, 3),
               encoder_jobs=jobs, encoder_threads_requested=(o.encoder_threads or 1) if encoder_env is not None else None, encoder_cpu_budget=o.cpu_budget,
               segments=len(segs), seg_frames=o.seg, file_bytes=size, video_bytes=vbytes, audio_payload_bytes=abytes,
               mb_per_hour=round(size / 1e6 / (secs / 3600), 1), video_kbit_s=round(vbytes * 8 / secs / 1000, 1),
               total_kbit_s=round(size * 8 / secs / 1000, 1), worst_second_bytes=chunk_rate(o.out, win),
               chunk_rate_limit=limit + 65536, max_picture_bytes=max(lens_all), pictures_raised_for_level=raised,
               joint_reencodes=reenc, scene_cut_i=cuts, valid=chk.returncode == 0, encode_seconds=round(time.time() - t0))

    # 4. quality
    if o.quality:
        log('measuring quality')
        qs = list(pool.map(lambda i: quality(segs[i] + '.y4m', segs[i] + '.tvc', work), range(len(segs))))
        wsum = sum(seg_frames)
        rep['vmaf'] = round(sum(q[0] * k for q, k in zip(qs, seg_frames)) / wsum, 2)
        rep['psnr_y'] = round(sum(q[1] * k for q, k in zip(qs, seg_frames)) / wsum, 2)
        rep['ssim_y'] = round(sum(q[2] * k for q, k in zip(qs, seg_frames)) / wsum, 4)
        rep['worst_segment_vmaf'] = round(min(q[0] for q in qs), 2)
        rep['segment_vmaf'] = [round(q[0], 2) for q in qs]
    rep['total_seconds'] = round(time.time() - t0)
    json.dump(rep, open(os.path.splitext(o.out)[0] + '.json', 'w'), indent=1)
    log(json.dumps({k: v for k, v in rep.items() if k != 'segment_vmaf'}))
    if not o.keep and not o.work:
        shutil.rmtree(work)
    sys.exit(0 if rep['valid'] else 3)


if __name__ == '__main__':
    main()

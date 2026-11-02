<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  type Info = { width: number; height: number; fps_num: number; fps_den: number; frames: number; audio: string | null };
  let info: Info | null = null, path = '', playing = false, done = false, loading = false, playPending = false, error = '', frame = 0;
  let scale = 1;
  let canvas: HTMLCanvasElement, audio: HTMLAudioElement, generation = 0, playAttempt = 0, origin = 0, pausedAt = 0, running = false, runner: Promise<void> | null = null;
  async function choose() {
    loading = true;
    try {
      const file = await open({ multiple: false, filters: [{ name: 'Tau video', extensions: ['tnv3'] }] });
      if (!file) return;
      playAttempt++; playing = false; audio?.pause(); generation++;
      if (runner) await runner;
      running = false;
      info = await invoke<Info>('player_open', { path: file });
      path = file; frame = 0; done = false; error = ''; pausedAt = 0;
      if (audio) { audio.src = info.audio ? convertFileSrc(info.audio) : ''; audio.load(); }
      const ctx = canvas?.getContext('2d'); ctx?.clearRect(0, 0, canvas.width, canvas.height);
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }
  function draw(raw: Uint8Array, details: Info) {
    const w = details.width, h = details.height, cw = Math.ceil(w / 2), ch = Math.ceil(h / 2), area = w * h, chroma = cw * ch;
    if (raw.length !== area + 2 * chroma) throw new Error('Invalid decoded frame size.');
    const ctx = canvas.getContext('2d'); if (!ctx) throw new Error('Video display unavailable.');
    const image = ctx.createImageData(w, h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = y * w + x, j = Math.floor(y / 2) * cw + Math.floor(x / 2);
      const l = Math.max(0, raw[i] - 16), u = raw[area + j] - 128, v = raw[area + chroma + j] - 128;
      image.data[4*i] = (298*l + 409*v + 128) >> 8;
      image.data[4*i+1] = (298*l - 100*u - 208*v + 128) >> 8;
      image.data[4*i+2] = (298*l + 516*u + 128) >> 8;
      image.data[4*i+3] = 255;
    }
    ctx.putImageData(image, 0, 0);
  }
  const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
  async function run(token: number, details: Info) {
    try {
      while (token === generation && !done) {
        while (!playing && token === generation) await wait(50);
        if (token !== generation) return;
        const raw = new Uint8Array(await invoke<ArrayBuffer>('player_next'));
        if (token !== generation) return;
        if (!raw.length) { playing = false; done = true; audio.pause(); break; }
        const due = frame * details.fps_den / details.fps_num;
        let clock = () => details.audio ? audio.currentTime : (performance.now() - origin) / 1000;
        while (token === generation && (!playing || clock() < due)) await wait(10);
        if (token !== generation) return;
        if (clock() - due < 0.25) draw(raw, details);
        frame++;
      }
    } catch (e) { if (token === generation) { error = String(e); playing = false; audio.pause(); } }
  }
  async function toggle() {
    if (!info || done || loading || playPending) return;
    if (playing) { playAttempt++; playing = false; if (info.audio) audio.pause(); else pausedAt = (performance.now() - origin) / 1000; return; }
    const attempt = ++playAttempt;
    playPending = true;
    try {
      if (info.audio) await audio.play(); else origin = performance.now() - pausedAt * 1000;
      if (attempt !== playAttempt) { audio?.pause(); return; }
      playing = true;
      if (!running) {
        running = true;
        const token = ++generation;
        const task = run(token, info);
        runner = task;
        void task.finally(() => { if (runner === task) { runner = null; running = false; } });
      }
    } catch (e) { if (attempt === playAttempt) error = `Could not play audio: ${String(e)}`; }
    finally { playPending = false; }
  }
  export function pausePlayback() {
    playAttempt++;
    audio?.pause();
    if (!playing || !info) return;
    playing = false;
    if (!info.audio) pausedAt = (performance.now() - origin) / 1000;
  }
</script>
<section aria-label="Local preview" class="preview">
  <h2>Local preview</h2><p class="help">Open a TNV3 file to preview it with its matching MP3 sidecar.</p>
  <div class="player-actions"><button type="button" onclick={choose} disabled={loading}>{loading ? 'Loading…' : 'Open video'}</button><button type="button" onclick={toggle} disabled={!info || done || loading || playPending}>{playPending ? 'Starting…' : playing ? 'Pause' : 'Play'}</button></div>
  {#if info}<p class="help filename">{path.split(/[\\/]/).pop()} · {info.audio ? 'MP3 sidecar found' : 'No MP3 sidecar — silent'}</p>{/if}
  <label class="preview-scale" for="preview-scale">Display scale <select id="preview-scale" bind:value={scale}><option value={1}>1× — actual size</option><option value={2}>2×</option><option value={4}>4×</option></select></label>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex: the viewport needs keyboard focus for scrolling. -->
  <div class="preview-viewport" tabindex="0" role="region" aria-label="Video preview; scroll to see enlarged video"><canvas bind:this={canvas} width={info?.width || 400} height={info?.height || 226} style:width="{(info?.width || 400) * scale}px" style:height="{(info?.height || 226) * scale}px" aria-label="Decoded video preview"></canvas></div>
  <audio bind:this={audio} preload="metadata"></audio>
  {#if info}<p class="help">Frame {frame} / {info.frames}{done ? ' · Finished' : ''}. Seeking is not available yet.</p>{/if}
  {#if error}<p class="status error" role="alert">{error}</p>{/if}
</section>

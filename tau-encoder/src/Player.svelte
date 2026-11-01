<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  type Info = { width: number; height: number; fps_num: number; fps_den: number; frames: number; audio: string | null };
  let info: Info | null = null, path = '', playing = false, done = false, error = '', frame = 0;
  let canvas: HTMLCanvasElement, audio: HTMLAudioElement, generation = 0, origin = 0, pausedAt = 0, running = false;
  async function choose() {
    try {
      const file = await open({ multiple: false, filters: [{ name: 'Tau video', extensions: ['tnv3'] }] });
      if (!file) return;
      generation++; playing = false; running = false; audio?.pause();
      info = await invoke<Info>('player_open', { path: file });
      path = file; frame = 0; done = false; error = ''; pausedAt = 0;
      if (audio) { audio.src = info.audio ? convertFileSrc(info.audio) : ''; audio.load(); }
      const ctx = canvas?.getContext('2d'); ctx?.clearRect(0, 0, canvas.width, canvas.height);
    } catch (e) { error = String(e); }
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
    } catch (e) { if (token === generation) { error = String(e); playing = false; audio.pause(); } } finally { if (token === generation) running = false; }
  }
  async function toggle() {
    if (!info || done) return;
    if (playing) { playing = false; if (info.audio) audio.pause(); else pausedAt = (performance.now() - origin) / 1000; return; }
    try {
      if (info.audio) await audio.play(); else origin = performance.now() - pausedAt * 1000;
      playing = true;
      if (!running) { running = true; const token = ++generation; void run(token, info); }
    } catch (e) { error = `Could not play audio: ${String(e)}`; }
  }
</script>
<section aria-label="Local preview" class="preview">
  <h2>Local preview</h2><p class="help">Open a TNV3 file to preview it with its matching MP3 sidecar.</p>
  <div class="player-actions"><button type="button" onclick={choose}>Open video</button><button type="button" onclick={toggle} disabled={!info || done}>{playing ? 'Pause' : 'Play'}</button></div>
  {#if info}<p class="help filename">{path.split(/[\\/]/).pop()} · {info.audio ? 'MP3 sidecar found' : 'No MP3 sidecar — silent'}</p>{/if}
  <canvas bind:this={canvas} width={info?.width || 400} height={info?.height || 226} aria-label="Decoded video preview"></canvas>
  <audio bind:this={audio} preload="metadata"></audio>
  {#if info}<p class="help">Frame {frame} / {info.frames}{done ? ' · Finished' : ''}. Seeking is not available yet.</p>{/if}
  {#if error}<p class="status error" role="alert">{error}</p>{/if}
</section>

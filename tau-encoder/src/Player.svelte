<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  type Info = { width: number; height: number; fps_num: number; fps_den: number; frames: number; audio: string | null; audio_source: string | null; subtitles: SubtitleTrack[] };
  type SubtitleTrack = { index: number; language: string; default: boolean; forced_track: boolean; variants: [number, number][] };
  let info: Info | null = null, path = '', playing = false, done = false, loading = false, playPending = false, error = '', frame = 0;
  // Display scale: "fit" (default) makes the output window as large as the available space allows; or a fixed factor.
  let scaleChoice: string = 'fit', viewportW = 0, windowH = 0;
  // Subtitles: the output window is 800x720 with the picture centred at its own size (container spec 6.4); the
  // overlay is drawn by the backend from the file's subtitle area and refreshed from the playback clock.
  const GRID_W = 800, GRID_H = 720;
  let subOn = true, subTrack = 0, subSize = 0, hideSdh = false, overlay: HTMLCanvasElement;
  let knownKey = 0, overlayBusy = false;
  $: stage = (info?.subtitles?.length ?? 0) > 0;
  // Crop (default): drop the empty black margins of the 800x720 window: keep the picture's width plus room for subtitles, from just above the picture down to the bottom.
  let crop = true;
  $: cropX = stage && crop ? Math.max(0, picLeft - 120) : 0;
  $: cropY = stage && crop ? Math.max(0, picTop - 12) : 0;
  $: cropW = stage && crop ? Math.min(GRID_W - cropX, (info?.width || 400) + 240) : stageW;
  $: cropH = stage && crop ? GRID_H - cropY : stageH;
  $: fitScale = Math.max(0.25, Math.min(viewportW > 0 ? viewportW / cropW : 1, windowH > 0 ? Math.max(200, windowH - 230) / cropH : 1));
  $: scale = scaleChoice === 'fit' ? fitScale : Number(scaleChoice);
  $: stageW = stage ? GRID_W : (info?.width || 400);
  $: stageH = stage ? GRID_H : (info?.height || 226);
  $: picLeft = stage ? Math.floor((GRID_W - (info?.width || 400)) / 2) : 0;
  $: picTop = stage ? Math.floor((GRID_H - (info?.height || 226)) / 2) : 0;
  function resetOverlay() { knownKey = 0; }
  function clockSeconds(): number {
    if (!info) return 0;
    if (info.audio) return audio?.currentTime ?? 0;
    return playing ? (performance.now() - origin) / 1000 : pausedAt;
  }
  async function refreshOverlay() {
    if (!info || !stage || !overlay || overlayBusy) return;
    overlayBusy = true;
    try {
      const raw = new Uint8Array(await invoke<ArrayBuffer>('player_subtitle', {
        track: subTrack, variant: subSize, position: Math.round(clockSeconds() * 48000),
        subtitlesOn: subOn, hideSdh, knownKey,
      }));
      const view = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
      const key = view.getUint32(0, true);
      if (raw.length > 4 && key !== knownKey) {
        const [x, y, w, h] = [view.getUint16(4, true), view.getUint16(6, true), view.getUint16(8, true), view.getUint16(10, true)];
        const ctx = overlay.getContext('2d');
        ctx?.clearRect(0, 0, GRID_W, GRID_H);
        if (ctx && w > 0 && h > 0) ctx.putImageData(new ImageData(new Uint8ClampedArray(raw.buffer.slice(raw.byteOffset + 12, raw.byteOffset + 12 + w * h * 4)), w, h), x, y);
        knownKey = key;
      }
    } catch (e) { error = String(e); }
    finally { overlayBusy = false; }
  }
  onMount(() => { const timer = setInterval(() => { void refreshOverlay(); }, 60); return () => clearInterval(timer); });
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
      subOn = true; hideSdh = false; knownKey = 0;
      const tracks = info.subtitles;
      subTrack = Math.max(0, tracks.findIndex(t => t.default)); if (tracks.length && tracks[subTrack] === undefined) subTrack = 0;
      subSize = tracks.length ? Math.floor(tracks[subTrack].variants.length / 2) : 0;
      overlay?.getContext('2d')?.clearRect(0, 0, GRID_W, GRID_H);
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
<svelte:window bind:innerHeight={windowH} />
<section aria-label="Local preview" class="preview">
  <h2>Local preview</h2><p class="help">Open a TNV3 file to preview it with its matching MP3 sidecar.</p>
  <div class="player-actions"><button type="button" onclick={choose} disabled={loading}>{loading ? 'Loading…' : 'Open video'}</button><button type="button" onclick={toggle} disabled={!info || done || loading || playPending}>{playPending ? 'Starting…' : playing ? 'Pause' : 'Play'}</button></div>
  {#if info}<p class="help filename">{path.split(/[\\/]/).pop()} · {info.audio_source === 'embedded-tac' ? 'Embedded TAC audio' : info.audio_source === 'mp3-sidecar' ? 'MP3 sidecar found' : 'No audio — silent'}</p>{/if}
  {#if info && stage}
    <div class="subtitle-controls" role="group" aria-label="Subtitles">
      <label><input type="checkbox" bind:checked={subOn} onchange={resetOverlay} /> Subtitles</label>
      <label>Track <select bind:value={subTrack} onchange={() => { subSize = Math.min(subSize, info!.subtitles[subTrack].variants.length - 1); resetOverlay(); }}>{#each info.subtitles as t}<option value={t.index}>{t.language}{t.default ? ' (default)' : ''}{t.forced_track ? ' (forced only)' : ''}</option>{/each}</select></label>
      <label>Size <select bind:value={subSize} onchange={resetOverlay}>{#each info.subtitles[subTrack].variants as v, i}<option value={i}>{v[0]}×{v[1]}</option>{/each}</select></label>
      <label><input type="checkbox" bind:checked={hideSdh} onchange={resetOverlay} /> Hide sound descriptions</label>
    </div>
    <p class="help">Output window 800×720 with the picture centred, as the Pocket draws it. With Subtitles off only forced lines show. Delay, vertical offset and box settings are not available here.</p>
  {/if}
  <label class="preview-scale"><input type="checkbox" bind:checked={crop} /> Crop empty borders</label>
    <label class="preview-scale" for="preview-scale">Display scale <select id="preview-scale" bind:value={scaleChoice}><option value="fit">Fit the window (default)</option><option value="0.5">0.5×</option><option value="1">1× — actual size</option><option value="2">2×</option><option value="4">4×</option></select></label>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex: the viewport needs keyboard focus for scrolling. -->
  <div class="preview-viewport" class:fit={scaleChoice === 'fit'} bind:clientWidth={viewportW} tabindex="0" role="region" aria-label="Video preview; scroll to see enlarged video"><div class="cropbox" style:width="{cropW * scale}px" style:height="{cropH * scale}px"><div class="stage" class:crisp={scale >= 2} style:width="{stageW * scale}px" style:height="{stageH * scale}px" style:left="{-cropX * scale}px" style:top="{-cropY * scale}px"><canvas bind:this={canvas} width={info?.width || 400} height={info?.height || 226} style:left="{picLeft * scale}px" style:top="{picTop * scale}px" style:width="{(info?.width || 400) * scale}px" style:height="{(info?.height || 226) * scale}px" aria-label="Decoded video preview"></canvas><canvas class="overlay" bind:this={overlay} width={GRID_W} height={GRID_H} style:display={stage ? 'block' : 'none'} style:width="{GRID_W * scale}px" style:height="{GRID_H * scale}px" aria-hidden="true"></canvas></div></div></div>
  <audio bind:this={audio} preload="metadata"></audio>
  {#if info}<p class="help">Frame {frame} / {info.frames}{done ? ' · Finished' : ''}. Seeking is not available yet.</p>{/if}
  {#if error}<p class="status error" role="alert">{error}</p>{/if}
</section>

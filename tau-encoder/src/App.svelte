<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  let input = '', folder = '', name = '', qp = 30, busy = false, message = '', error = '';
  type ResultInfo = { frames: number; level: number; reencoded: number; video: string; audio: string };
  onMount(() => { try { folder = localStorage.getItem('tau-encoder-output-folder') || ''; } catch { /* Storage may be unavailable. */ } });
  async function chooseInput() {
    try {
      const file = await open({ multiple: false, directory: false, filters: [{ name: 'Video', extensions: ['mp4', 'mkv', 'mov', 'avi', 'webm', 'm4v', 'mpg', 'mpeg', 'y4m'] }, { name: 'All files', extensions: ['*'] }] });
      if (file) { input = file; name = file.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || 'video'; message = ''; error = ''; }
    } catch (e) { error = String(e); }
  }
  async function chooseFolder() {
    try {
      const dir = await open({ multiple: false, directory: true, defaultPath: folder || undefined });
      if (dir) { folder = dir; try { localStorage.setItem('tau-encoder-output-folder', folder); } catch { /* Keep the current selection. */ } message = ''; error = ''; }
    } catch (e) { error = String(e); }
  }
  async function encode() {
    if (!input || !folder || !name || busy) return;
    busy = true; error = ''; message = '';
    try {
      const result = await invoke<ResultInfo>('encode', { input, folder, name, qp: Number(qp) });
      message = `Finished: ${result.frames} frames · level ${result.level} · ${result.reencoded} frames retried. Created ${result.video} and ${result.audio}.`;
    } catch (e) { error = String(e); } finally { busy = false; }
  }
</script>
<svelte:head><title>Tau Encoder</title></svelte:head>
<main>
  <header><span class="mark">τ</span><div><p class="eyebrow">TAU VIDEO TOOLS</p><h1>Tau Encoder</h1></div><span class="badge">PREVIEW</span></header>
  <p class="intro">Convert a video into a Tau video and matching audio file.</p>
  <section aria-label="Video settings">
    <div class="field"><label for="input">Source video <small>MP4, MKV, MOV and more</small></label><div class="pick"><input id="input" value={input} readonly placeholder="Choose a video file" /><button type="button" onclick={chooseInput} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="folder">Output folder <small>Choose a folder or card</small></label><div class="pick"><input id="folder" value={folder} readonly placeholder="Choose where to save" /><button type="button" onclick={chooseFolder} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="name">File name <small>Creates matching .tnv3 and .mp3 files</small></label><input class="name" id="name" type="text" bind:value={name} disabled={busy} placeholder="Video name" /></div>
    <div class="field quality"><div><label for="qp">Quality <small>QP</small></label><p>Lower values use more space. Default: 30.</p></div><input id="qp" type="number" min="0" max="51" step="1" bind:value={qp} disabled={busy} /></div>
    <button class="primary" type="button" onclick={encode} disabled={busy || !input || !folder || !name.trim() || qp == null || qp < 0 || qp > 51 || !Number.isInteger(qp)}>{busy ? 'Encoding…' : 'Encode video and audio'}</button>
    {#if busy}<p class="status" role="status">Encoding is running. Keep the app open until it finishes.</p>{/if}
    {#if message}<p class="status success" role="status">{message}</p>{/if}
    {#if error}<p class="status error" role="alert">{error}</p>{/if}
  </section>
  <aside><strong>Early preview</strong><p>Output is 24 fps, up to 400 × 360, with 48 kHz stereo MP3. Some videos may fail or require lower quality. Progress and cancellation are not available yet.</p></aside>
</main>

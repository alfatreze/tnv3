<script lang="ts">
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  let input = '', output = '', qp = 30, busy = false, message = '', error = '';
  type ResultInfo = { frames: number; level: number; reencoded: number; bytes: number };
  async function chooseInput() {
    const file = await open({ multiple: false, directory: false, filters: [{ name: 'Y4M video', extensions: ['y4m'] }] });
    if (file) { input = file; message = ''; error = ''; }
  }
  async function chooseOutput() {
    const file = await save({ defaultPath: 'video.tnv3', filters: [{ name: 'Tau video', extensions: ['tnv3'] }] });
    if (file) { output = file; message = ''; error = ''; }
  }
  async function encode() {
    if (!input || !output || busy) return;
    busy = true; error = ''; message = '';
    try {
      const result = await invoke<ResultInfo>('encode', { input, output, qp: Number(qp) });
      message = `Finished: ${result.frames} frames · level ${result.level} · ${(result.bytes / 1048576).toFixed(2)} MB · ${result.reencoded} frames retried at higher QP.`;
    } catch (e) { error = String(e); } finally { busy = false; }
  }
</script>
<svelte:head><title>Tau Encoder</title></svelte:head>
<main>
  <header><span class="mark">τ</span><div><p class="eyebrow">TAU VIDEO TOOLS</p><h1>Tau Encoder</h1></div><span class="badge">PREVIEW</span></header>
  <p class="intro">Turn a Y4M video into a Tau video file.</p>
  <section aria-label="Video settings">
    <div class="field"><label for="input">Source video <small>Y4M · 8-bit YUV 4:2:0</small></label><div class="pick"><input id="input" value={input} readonly placeholder="Choose a Y4M file" /><button type="button" onclick={chooseInput} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="output">Save as <small>TNV3 · video only</small></label><div class="pick"><input id="output" value={output} readonly placeholder="Choose where to save" /><button type="button" onclick={chooseOutput} disabled={busy}>Browse</button></div></div>
    <div class="field quality"><div><label for="qp">Quality <small>QP</small></label><p>Lower values use more space. Default: 30.</p></div><input id="qp" type="number" min="0" max="51" step="1" bind:value={qp} disabled={busy} /></div>
    <button class="primary" type="button" onclick={encode} disabled={busy || !input || !output || qp == null || qp < 0 || qp > 51 || !Number.isInteger(qp)}>{busy ? 'Encoding…' : 'Encode video'}</button>
    {#if busy}<p class="status" role="status">Encoding is running. Keep the app open until it finishes.</p>{/if}
    {#if message}<p class="status success" role="status">{message}</p>{/if}
    {#if error}<p class="status error" role="alert">{error}</p>{/if}
  </section>
  <aside><strong>Early preview</strong><p>The encoder uses a conservative rate limit and may lower quality or fail on some videos. Audio is not included; MP3 sidecars are a separate tool. Progress and cancellation are not available yet.</p></aside>
</main>

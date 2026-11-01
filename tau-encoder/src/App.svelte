<script lang="ts">
  import { onMount } from 'svelte';
  import Player from './Player.svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  let input = '', folder = '', name = '', qp = 30, busy = false, message = '', error = '', warning = '';
  let mode: 'rust' | 'c' = 'rust', suffix = '_R';
  let roundDown = false, eofPass = false, trimMp3Delay = false;
  type DependencyKey = 'ffmpeg' | 'ffprobe' | 'python' | 'script' | 'tvcEncoder' | 'tnv3Tool';
  let dependencies: Record<DependencyKey, string> = { ffmpeg: '', ffprobe: '', python: '', script: '', tvcEncoder: '', tnv3Tool: '' };
  const dependencyFields: { key: DependencyKey; label: string; help: string }[] = [
    { key: 'ffmpeg', label: 'FFmpeg', help: 'Video decoding and MP3 encoding, both encoders' },
    { key: 'ffprobe', label: 'FFprobe', help: 'Source inspection for the C encoder' },
    { key: 'python', label: 'Python 3', help: 'Runs the C encoder script' },
    { key: 'script', label: 'C encoder script', help: 'Select encode_film.py' },
    { key: 'tvcEncoder', label: 'TVC encoder', help: 'Select tvc_enc (or tvc_enc.exe)' },
    { key: 'tnv3Tool', label: 'TNV3 tool', help: 'Select tnv3 (or tnv3.exe)' },
  ];
  function saveDependencies() {
    try { localStorage.setItem('tau-encoder-dependencies', JSON.stringify(dependencies)); } catch { /* Keep settings for this session. */ }
  }
  async function chooseDependency(key: DependencyKey) {
    try {
      const file = await open({ multiple: false, directory: false, defaultPath: dependencies[key] || undefined });
      if (file) { dependencies = { ...dependencies, [key]: file }; saveDependencies(); }
    } catch (e) { error = String(e); }
  }
  type Progress = { phase: 'probing' | 'video' | 'c-video' | 'audio' | 'saving'; frames: number; total_frames_estimate: number | null; remaining_seconds: number | null };
  let progress: Progress = { phase: 'probing', frames: 0, total_frames_estimate: null, remaining_seconds: null };
  function timeLabel(seconds: number): string {
    if (seconds < 60) return `${seconds} sec`;
    if (seconds < 3600) return `${Math.ceil(seconds / 60)} min`;
    const minutes = Math.ceil(seconds / 60);
    return `${Math.floor(minutes / 60)} hr ${minutes % 60} min`;
  }
  function progressDetail(p: Progress): string {
    if (p.phase === 'c-video') return p.total_frames_estimate !== null ? `Encoded ${p.frames} of ${p.total_frames_estimate} segments. Time remaining is not available yet.` : 'Preparing and encoding segments. Keep the app open.';
    if (p.phase !== 'video') return 'Keep the app open until it finishes.';
    const count = `${p.frames} ${p.frames === 1 ? 'frame' : 'frames'} processed`;
    const total = p.total_frames_estimate !== null && p.frames < p.total_frames_estimate ? ` of about ${p.total_frames_estimate}` : '';
    const estimate = p.remaining_seconds !== null ? `About ${timeLabel(p.remaining_seconds)} remaining for video.` : p.total_frames_estimate === null ? 'Video duration is unavailable.' : p.frames >= p.total_frames_estimate ? 'Continuing beyond the estimated video length.' : 'Estimating video time…';
    return `${count}${total}. ${estimate}`;
  }
  type ResultInfo = { frames: number; level: number; reencoded: number; video: string; audio: string; warnings: string[] };
  onMount(() => { try {
    folder = localStorage.getItem('tau-encoder-output-folder') || '';
    const saved = JSON.parse(localStorage.getItem('tau-encoder-dependencies') || '{}');
    for (const field of dependencyFields) if (typeof saved?.[field.key] === 'string') dependencies[field.key] = saved[field.key];
  } catch { /* Storage may be unavailable or invalid. */ } });
  function changeMode(event: Event) {
    mode = (event.currentTarget as HTMLSelectElement).value as 'rust' | 'c';
    suffix = mode === 'rust' ? '_R' : '_C';
  }
  async function chooseInput() {
    try {
      const file = await open({ multiple: false, directory: false, filters: [{ name: 'Video', extensions: ['mp4', 'mkv', 'mov', 'avi', 'webm', 'm4v', 'mpg', 'mpeg', 'y4m'] }, { name: 'All files', extensions: ['*'] }] });
      if (file) { input = file; name = file.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || 'video'; message = ''; error = ''; warning = ''; }
    } catch (e) { error = String(e); }
  }
  async function chooseFolder() {
    try {
      const dir = await open({ multiple: false, directory: true, defaultPath: folder || undefined });
      if (dir) { folder = dir; try { localStorage.setItem('tau-encoder-output-folder', folder); } catch { /* Keep the current selection. */ } message = ''; error = ''; warning = ''; }
    } catch (e) { error = String(e); }
  }
  async function encode() {
    if (!input || !folder || !name || busy) return;
    busy = true; error = ''; message = ''; warning = '';
    progress = { phase: 'probing', frames: 0, total_frames_estimate: null, remaining_seconds: null };
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<Progress>('encode-progress', event => { progress = event.payload; });
      const result = await invoke<ResultInfo>('encode', { input, folder, name: name + suffix, qp: Number(qp), mode, advanced: { roundDown, eofPass: roundDown && eofPass, trimMp3Delay }, dependencies });
      warning = result.warnings.join('\n\n');
      message = `Finished: ${result.frames} frames · level ${result.level} · ${result.reencoded} frames retried. Created ${result.video} and ${result.audio}.`;
    } catch (e) { error = String(e); } finally { unlisten?.(); busy = false; }
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
    <div class="field"><label for="mode">Encoder</label><select id="mode" value={mode} onchange={changeMode} disabled={busy}><option value="rust">Rust encoder</option><option value="c">C encoder</option></select></div>
    <div class="field"><label for="suffix">Output suffix <small>Added to both file names</small></label><input class="name" id="suffix" bind:value={suffix} disabled={busy} /></div>
    <div class="field quality"><div><label for="qp">Quality <small>QP</small></label><p>Lower values use more space. Default: 30.</p></div><input id="qp" type="number" min="0" max="51" step="1" bind:value={qp} disabled={busy} /></div>
    <details class="advanced dependency-settings">
      <summary>Dependency settings</summary>
      <p>Choose installed files to override automatic discovery. Leave a path empty to use the bundled or default tool. These settings are remembered on this computer.</p>
      {#each dependencyFields as field}
        <div class="field"><label for={'dependency-' + field.key}>{field.label} <small>{field.help}</small></label>
          <div class="pick"><input id={'dependency-' + field.key} bind:value={dependencies[field.key]} onchange={saveDependencies} disabled={busy} placeholder="Automatic" /><button type="button" onclick={() => chooseDependency(field.key)} disabled={busy}>Browse</button></div>
        </div>
      {/each}
    </details>
    <details class="advanced">
      <summary>Advanced Pocket experiments</summary>
      <p>Optional tests for both encoders. Change one setting at a time and use a different suffix to compare files. These have not been verified on Pocket.</p>
      <label><input type="checkbox" bind:checked={roundDown} disabled={busy} /> Round frame timestamps down</label>
      <p>May reduce repeated frames for some sources; can shift timing or omit the last frame.</p>
      <label><input type="checkbox" bind:checked={eofPass} disabled={busy || !roundDown} /> Pass the final frame with round-down</label>
      <p>May restore a final frame, but can add an extra frame.</p>
      <label><input type="checkbox" bind:checked={trimMp3Delay} disabled={busy} /> Test MP3 delay compensation</label>
      <p>Removes the first 1,105 audio samples (23 ms) before encoding. This may improve or worsen sync and permanently removes that audio.</p>
    </details>
    <button class="primary" type="button" onclick={encode} disabled={busy || !input || !folder || !name.trim() || qp == null || qp < 0 || qp > 51 || !Number.isInteger(qp)}>{busy ? 'Encoding…' : 'Encode video and audio'}</button>
    {#if busy}
      <div class="progress-area" aria-live="polite">
        <div class="progress-heading"><strong>{progress.phase === 'probing' ? 'Reading video information' : progress.phase === 'video' ? 'Decoding and encoding video' : progress.phase === 'c-video' ? 'Encoding video with C' : progress.phase === 'audio' ? 'Encoding audio' : 'Saving files'}</strong>
          {#if progress.phase === 'video' && progress.total_frames_estimate && progress.frames < progress.total_frames_estimate}<span>~{Math.min(99, Math.floor(progress.frames / progress.total_frames_estimate * 100))}%</span>{/if}
        </div>
        {#if progress.phase === 'video' && progress.total_frames_estimate && progress.frames < progress.total_frames_estimate}
          <progress aria-label="Estimated video progress" value={progress.frames} max={progress.total_frames_estimate}></progress>
        {:else}
          <progress aria-label="Encoding progress"></progress>
        {/if}
        <p class="progress-detail">{progressDetail(progress)}</p>
      </div>
    {/if}
    {#if message}<p class="status success" role="status">{message}</p>{/if}
    {#if warning}<p class="status warning" role="alert">{warning}</p>{/if}
    {#if error}<p class="status error" role="alert">{error}</p>{/if}
  </section>
  <Player />
  <aside><strong>Early preview</strong><p>Output is 24 fps, up to 400 × 360, with 48 kHz stereo MP3. Some videos may fail or require lower quality. Time estimates cover video processing only. Cancellation is not available yet.</p></aside>
</main>

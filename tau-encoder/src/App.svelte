<script lang="ts">
  import { onMount } from 'svelte';
  import Player from './Player.svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { version } from '../package.json';
  let input = '', folder = '', name = '', qp = 30, busy = false, message = '', error = '', warning = '';
  let dependencyError = '';
  let mode: 'rust' | 'c' = 'c', suffix = '_C', rustAvailable = false;
  let resolution = '400x360', customWidth = 400, customHeight = 360, selectedLevel = 0;
  function outputDimensions(): [number, number] { return resolution === 'custom' ? [Number(customWidth), Number(customHeight)] : resolution.split('x').map(Number) as [number, number]; }
  function validDimensions(): boolean { const [w, h] = outputDimensions(); return Number.isInteger(w) && Number.isInteger(h) && w >= 2 && w <= 800 && h >= 2 && h <= 720 && w % 2 === 0 && h % 2 === 0; }
  let workflow: 'python' | 'native' = 'python', workspaceMib = 512;
  let page: 'encoder' | 'decoder' | 'settings' = 'encoder';
  let player: { pausePlayback: () => void } | undefined;
  let roundDown = false, eofPass = false, trimMp3Delay = false;
  type DependencyKey = 'ffmpeg' | 'ffprobe' | 'python' | 'script' | 'tvcEncoder' | 'tnv3Tool';
  let dependencies: Record<DependencyKey, string> = { ffmpeg: '', ffprobe: '', python: '', script: '', tvcEncoder: '', tnv3Tool: '' };
  type ToolDiscovery = { key: DependencyKey; automatic: string; candidates: string[] };
  let discovered: ToolDiscovery[] = [], discoveryError = '', discovering = false, discoveryRequest = 0;
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
    void refreshTools();
  }
  async function refreshTools() {
    const request = ++discoveryRequest;
    discovering = true; discoveryError = '';
    try {
      const result = await invoke<ToolDiscovery[]>('discover_tools', { dependencies, workflow });
      if (request === discoveryRequest) discovered = result;
    } catch (e) { if (request === discoveryRequest) discoveryError = `Could not find tools: ${String(e)}`; }
    finally { if (request === discoveryRequest) discovering = false; }
  }
  function chooseDiscovered(key: DependencyKey, event: Event) {
    dependencies = { ...dependencies, [key]: (event.currentTarget as HTMLSelectElement).value };
    saveDependencies();
  }
  async function chooseDependency(key: DependencyKey) {
    dependencyError = '';
    try {
      const file = await open({ multiple: false, directory: false, defaultPath: dependencies[key] || undefined });
      if (file) { dependencies = { ...dependencies, [key]: file }; saveDependencies(); }
    } catch (e) { dependencyError = String(e); }
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
  function warningExplanations(text: string): string[] {
    const lower = text.toLowerCase();
    const explanations: string[] = [];
    if (lower.includes('[matroska,webm') && (lower.includes('invalid as first byte') || lower.includes('duplicate element') || lower.includes('exceeds containing master'))) {
      explanations.push('The source container has malformed or duplicate elements. FFmpeg continued reading it, but video or audio data may have been skipped or damaged; container warnings alone do not prove an audio decoding fault.');
    }
    if (lower.includes('could not find ref with poc')) {
      explanations.push('The HEVC decoder could not find reference pictures. Affected frames may have visible corruption, missing detail, or discontinuities.');
    }
    if (!explanations.length) explanations.push('FFmpeg reported errors while reading the source. The video or audio may be affected; consult the details below.');
    explanations.push('The app did not repair or verify the affected source data. Check the output, and use a clean source if available.');
    return explanations;
  }
  type ResultInfo = { frames: number; level: number; reencoded: number; video: string; audio: string; warnings: string[] };
  onMount(() => { void invoke<boolean>('experimental_rust_encoder_available').then(value => { rustAvailable = value; }).catch(() => { rustAvailable = false; }); try {
    folder = localStorage.getItem('tau-encoder-output-folder') || '';
    const saved = JSON.parse(localStorage.getItem('tau-encoder-dependencies') || '{}');
    for (const field of dependencyFields) if (typeof saved?.[field.key] === 'string') dependencies[field.key] = saved[field.key];
  } catch { /* Storage may be unavailable or invalid. */ } });
  function changeMode(event: Event) {
    mode = (event.currentTarget as HTMLSelectElement).value as 'rust' | 'c';
    suffix = mode === 'rust' ? '_R' : '_C';
  }
  function navigate(next: 'encoder' | 'decoder' | 'settings') {
    if (page === 'decoder' && next !== 'decoder') player?.pausePlayback();
    page = next;
    if (next === 'settings') void refreshTools();
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
    if (!input || !folder || !name || busy || !validDimensions() || (mode === 'c' && workflow === 'native' && (!Number.isInteger(Number(workspaceMib)) || Number(workspaceMib) < 1 || Number(workspaceMib) > 16_384))) return;
    busy = true; error = ''; message = ''; warning = '';
    progress = { phase: 'probing', frames: 0, total_frames_estimate: null, remaining_seconds: null };
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<Progress>('encode-progress', event => { progress = event.payload; });
      const [width, height] = outputDimensions();
      const result = await invoke<ResultInfo>('encode', { input, folder, name: name + suffix, qp: Number(qp), mode, workflow: mode === 'rust' ? null : workflow, workspaceMib: Number(workspaceMib), advanced: { roundDown, eofPass: roundDown && eofPass, trimMp3Delay, width, height, level: mode === 'rust' ? 0 : Number(selectedLevel) }, dependencies });
      warning = result.warnings.join('\n\n');
      message = `Finished: ${result.frames} frames · level ${result.level}. Created ${result.video} and ${result.audio}. Level limits may cause automatic quality reductions.`;
    } catch (e) { error = String(e); } finally { unlisten?.(); busy = false; }
  }
</script>
<svelte:head><title>Tau Encoder</title></svelte:head>
<div class="app-shell">
  <nav class="side-nav" aria-label="Main navigation">
    <div class="nav-brand"><span class="mark">τ</span><div><p class="eyebrow">TAU VIDEO TOOLS</p><strong>Tau Encoder</strong></div></div>
    <div class="nav-items" role="group" aria-label="Pages">
      <button type="button" class:active={page === 'encoder'} aria-current={page === 'encoder' ? 'page' : undefined} onclick={() => navigate('encoder')}>Encoder</button>
      <button type="button" class:active={page === 'decoder'} aria-current={page === 'decoder' ? 'page' : undefined} onclick={() => navigate('decoder')}>Decoder</button>
      <button type="button" class:active={page === 'settings'} aria-current={page === 'settings' ? 'page' : undefined} onclick={() => navigate('settings')}>Settings</button>
    </div>
    <span class="badge">PREVIEW · v{version}</span>
  </nav>
  <span class="sr-only" aria-live="polite" aria-atomic="true">{page === 'encoder' ? 'Encoder page' : page === 'decoder' ? 'Decoder page' : 'Settings page'}</span>
  <main>
  {#if page === 'encoder'}
  <header><div><p class="eyebrow">VIDEO CONVERSION</p><h1>Encoder</h1></div></header>
  <p class="intro">Convert a video into a Tau video and matching audio file.</p>
  <section aria-label="Video settings">
    <div class="field"><label for="input">Source video <small>MP4, MKV, MOV and more</small></label><div class="pick"><input id="input" value={input} readonly placeholder="Choose a video file" /><button type="button" onclick={chooseInput} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="folder">Output folder <small>Choose a folder or card</small></label><div class="pick"><input id="folder" value={folder} readonly placeholder="Choose where to save" /><button type="button" onclick={chooseFolder} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="name">File name <small>Creates matching .tnv3 and .mp3 files</small></label><input class="name" id="name" type="text" bind:value={name} disabled={busy} placeholder="Video name" /></div>
    {#if rustAvailable}<div class="field"><label for="mode">Encoder</label><select id="mode" value={mode} onchange={changeMode} disabled={busy}><option value="c">C reference encoder</option><option value="rust">Rust encoder (experimental)</option></select></div>{/if}
    {#if mode === 'c'}
      <div class="field"><label for="workflow">C encoder workflow</label><select id="workflow" bind:value={workflow} onchange={() => void refreshTools()} disabled={busy}><option value="python">Python + C (default)</option><option value="native">Experimental Python-free Rust + C</option></select>
        {#if workflow === 'python'}<p class="workflow-note">Uses Python 3 to coordinate the segmented C video encoder. Requires Python in addition to the existing tools.</p>{:else}<p class="workflow-note">Rust handles orchestration; the same C video encoder still performs compression. Python is not required. This experimental workflow tracks a projected temporary workspace budget; long films may need a higher value. Encoding stops when the projected workspace exceeds this budget, though actual disk use can vary. Video progress updates as segments finish.</p>
          <label class="workspace-label" for="workspace-mib">Projected workspace budget <small>MiB</small></label><input class="workspace-input" id="workspace-mib" type="number" min="1" max="16384" step="1" bind:value={workspaceMib} disabled={busy} />
        {/if}
      </div>
    {/if}
    <div class="field"><label for="suffix">Output suffix <small>Added to both file names</small></label><input class="name" id="suffix" bind:value={suffix} disabled={busy} /></div>
    <div class="field"><label for="resolution">Output size <small>Maximum width × height</small></label><select id="resolution" bind:value={resolution} disabled={busy}><option value="400x240">400 × 240 — Level 1 size</option><option value="400x360">400 × 360 — Pocket window (default)</option><option value="800x464">800 × 464 — Level 2 size</option><option value="800x720">800 × 720 — Level 3 size</option><option value="custom">Custom</option></select>{#if resolution === 'custom'}<div class="dimensions"><label for="custom-width">Width <input id="custom-width" type="number" min="2" max="800" step="2" bind:value={customWidth} disabled={busy} /></label><label for="custom-height">Height <input id="custom-height" type="number" min="2" max="720" step="2" bind:value={customHeight} disabled={busy} /></label></div>{#if !validDimensions()}<p class="status error">Use even dimensions from 2 × 2 up to 800 × 720.</p>{/if}{/if}<p class="workflow-note">Preserves aspect ratio; actual dimensions may be smaller. Higher resolutions and levels are not confirmed on Pocket.</p></div>
    {#if mode === 'c'}<div class="field"><label for="level">Codec level</label><select id="level" bind:value={selectedLevel} disabled={busy}><option value={0}>Automatic — lowest compatible level</option><option value={1}>Level 1</option><option value={2}>Level 2</option><option value={3}>Level 3</option></select><p class="workflow-note">The encode fails if the selected level cannot hold the output. The encoder can raise QP on individual pictures to meet the level, reducing quality in those pictures.</p></div>{/if}
    <div class="field quality"><div><label for="qp">Quality <small>QP</small></label><p>Lower values use more space. Default: 30.</p></div><input id="qp" type="number" min="0" max="51" step="1" bind:value={qp} disabled={busy} /></div>
    <button class="primary" type="button" onclick={encode} disabled={busy || !input || !folder || !name.trim() || !validDimensions() || qp == null || qp < 0 || qp > 51 || !Number.isInteger(qp) || (mode === 'c' && workflow === 'native' && (!Number.isInteger(Number(workspaceMib)) || Number(workspaceMib) < 1 || Number(workspaceMib) > 16_384))}>{busy ? 'Encoding…' : 'Encode video and audio'}</button>
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
    {#if warning}<div class="status warning-summary" role="alert"><strong>Output created with source warnings</strong>{#each warningExplanations(warning) as explanation}<p>{explanation}</p>{/each}<details><summary>Show original decoder messages</summary><pre class="warning">{warning}</pre></details></div>{/if}
    {#if error}<p class="status error" role="alert">{error}</p>{/if}
  </section>
  <aside><strong>Early preview</strong><p>Output is 24 fps with 48 kHz stereo MP3. Some videos may fail or require lower quality. Time estimates cover video processing only. Cancellation is not available yet.</p></aside>
  {:else if page === 'decoder'}
    <header><div><p class="eyebrow">LOCAL PREVIEW</p><h1>Decoder</h1></div></header>
    <p class="intro">Test player for local TNV3 video and matching audio files.</p>
  {:else}
    <header><div><p class="eyebrow">APP CONFIGURATION</p><h1>Settings</h1></div></header>
    <p class="intro">Choose installed tools to override automatic discovery. Paths are remembered on this computer.</p>
    <section aria-label="Dependency settings">
      <p class="settings-copy">Leave a path empty to use the bundled or default tool. The Python path is used only by the Python + C workflow.</p>
      <p class="settings-copy">Detected paths depend on the app's environment and the selected workflow. File detection does not verify that a tool works or supports your source.</p>
      <button type="button" onclick={() => void refreshTools()} disabled={busy || discovering}>{discovering ? 'Scanning…' : 'Refresh detected tools'}</button>
      {#if discoveryError}<p class="status error" role="alert">{discoveryError}</p>{/if}
      {#each dependencyFields as field}
          {@const tool = discovered.find(item => item.key === field.key)}
          <div class="field"><label for={'dependency-' + field.key}>{field.label} <small>{field.help}</small></label>
            <div class="pick"><input id={'dependency-' + field.key} bind:value={dependencies[field.key]} onchange={saveDependencies} disabled={busy} placeholder="Automatic" /><button type="button" onclick={() => chooseDependency(field.key)} disabled={busy}>Browse</button></div>
            <p class="detected-path">{dependencies[field.key] ? `Selected: ${dependencies[field.key]}` : tool?.automatic ? `Automatic: ${tool.automatic}` : 'Automatic: not found (or not used by this workflow)'}</p>
            {#if tool && tool.candidates.length > 0}
              <label class="detected-label" for={'detected-' + field.key}>Detected {field.label} locations</label>
              <select id={'detected-' + field.key} value={dependencies[field.key]} onchange={(event) => chooseDiscovered(field.key, event)} disabled={busy}>
                <option value="">Automatic</option>
                {#if dependencies[field.key] && !tool.candidates.includes(dependencies[field.key])}<option value={dependencies[field.key]}>Custom: {dependencies[field.key]}</option>{/if}
                {#each tool.candidates as path}<option value={path}>{path}</option>{/each}
              </select>
            {/if}
          </div>
      {/each}
      {#if dependencyError}<p class="status error" role="alert">{dependencyError}</p>{/if}
    </section>
    <details class="advanced settings-advanced" open>
      <summary>Advanced Pocket experiments</summary>
      <p>Optional tests for both encoders. Change one setting at a time and use a different suffix to compare files. These have not been verified on Pocket.</p>
      <label><input type="checkbox" bind:checked={roundDown} disabled={busy} /> Round frame timestamps down</label>
      <p>May reduce repeated frames for some sources; can shift timing or omit the last frame.</p>
      <label><input type="checkbox" bind:checked={eofPass} disabled={busy || !roundDown} /> Pass the final frame with round-down</label>
      <p>May restore a final frame, but can add an extra frame.</p>
      <label><input type="checkbox" bind:checked={trimMp3Delay} disabled={busy} /> Test MP3 delay compensation</label>
      <p>Removes the first 1,105 audio samples (23 ms) before MP3 encoding. This may improve or worsen sync and permanently removes that audio.</p>
    </details>
  {/if}
  <div class="decoder-player" hidden={page !== 'decoder'}><Player bind:this={player} /></div>
</main>
</div>

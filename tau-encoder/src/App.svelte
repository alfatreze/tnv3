<script lang="ts">
  import { onMount } from 'svelte';
  import Player from './Player.svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { version } from '../package.json';
  let input = '', folder = '', name = '', qp = 30, busy = false, message = '', error = '', warning = '';
  let alwaysContinue = false, checkingSource = false, encodeRunning = false, encodeRegistered = false, stopRequested = false, sourceIssue = '', rememberDecision = false;
  let resolveSourceDecision: ((value: boolean) => void) | null = null;
  function focusDialogButton(node: HTMLButtonElement) { node.focus(); }
  function openModal(node: HTMLDialogElement) { node.showModal(); return { destroy: () => node.close() }; }
  type SourceCheckResult = { status: 'clean' | 'warning' | 'stopped'; details: string };
  type SourceDecisionEvent = { job_id: string; details: string };
  // Encode log: one detailed JSON-lines file per encode, kept in the app's log folder (default on).
  let keepLog = true, logDir = '', logError = '';
  function saveKeepLog() { try { localStorage.setItem('tau-encoder-keep-log', String(keepLog)); } catch { /* Keep this session's choice. */ } }
  async function openLogFolder() { logError = ''; try { await invoke('open_log_folder'); } catch (e) { logError = String(e); } }
  function saveRecoveryPreference() { try { localStorage.setItem('tau-encoder-always-continue-source-errors', String(alwaysContinue)); } catch { /* Keep this session's preference. */ } }
  function decideSource(keepGoing: boolean) {
    if (keepGoing && rememberDecision) { alwaysContinue = true; saveRecoveryPreference(); }
    sourceIssue = '';
    const resolve = resolveSourceDecision; resolveSourceDecision = null; resolve?.(keepGoing);
  }
  function askSourceDecision(details: string): Promise<boolean> {
    sourceIssue = details || 'FFmpeg reported a problem while reading this source.';
    rememberDecision = false;
    return new Promise(resolve => { resolveSourceDecision = resolve; });
  }
  async function stopChecking() { try { await invoke('stop_source_check'); } catch (e) { error = String(e); } }
  async function stopAfterPhase() { try { await invoke('stop_encode'); stopRequested = true; } catch (e) { error = String(e); } }
  let dependencyError = '';
  let mode: 'rust' | 'c' = 'c', suffix = '_C', rustAvailable = false;
  let resolution = '400', customWidth = 400, selectedLevel = 0;
  type OutputPreview = { width: number; height: number; minimum_level: number };
  let outputPreview: OutputPreview | null = null, previewError = '', previewLoading = false, previewRequest = 0;
  function outputWidth(): number { return resolution === 'custom' ? Number(customWidth) : Number(resolution); }
  function validDimensions(): boolean { const w = outputWidth(); return Number.isInteger(w) && w >= 2 && w <= 800 && w % 2 === 0; }
  function invalidLevel(): boolean { return mode === 'c' && Number(selectedLevel) > 0 && !!outputPreview && Number(selectedLevel) < outputPreview.minimum_level; }
  type AudioTrack = { index: number; codec: string; channels: number; language: string; title: string };
  type SubtitleTrack = { index: number; codec: string; language: string; title: string; text: boolean };
  let subtitleTracks: SubtitleTrack[] = [], subtitleTrack = -1;
  function subtitleLabel(t: SubtitleTrack): string { return `Track ${t.index + 1}: ${[t.language, t.title, t.codec].filter(Boolean).join(' · ')}${t.text ? '' : ' (image subtitles cannot be used)'}`; }
  let audioTracks: AudioTrack[] = [], audioTrack = 0, tracksRequest = 0;
  function trackLabel(t: AudioTrack): string { return `Track ${t.index + 1}: ${[t.language, t.title, t.codec, t.channels ? `${t.channels} ch` : ''].filter(Boolean).join(' · ')}`; }
  async function refreshTracks() {
    const request = ++tracksRequest;
    audioTracks = []; audioTrack = 0; subtitleTracks = []; subtitleTrack = -1;
    if (!input) return;
    try { const result = await invoke<AudioTrack[]>('list_audio_tracks', { input, dependencies }); if (request === tracksRequest) audioTracks = result; }
    catch { /* A source without readable track data encodes with its first audio track. */ }
    try { const result = await invoke<SubtitleTrack[]>('list_subtitle_tracks', { input, dependencies }); if (request === tracksRequest) subtitleTracks = result; }
    catch { /* No readable subtitle data: no subtitles offered. */ }
  }
  async function refreshOutputPreview() {
    const request = ++previewRequest;
    outputPreview = null; previewError = '';
    if (!input || !validDimensions()) { previewLoading = false; return; }
    previewLoading = true;
    try { const result = await invoke<OutputPreview>('inspect_output', { input, width: outputWidth(), dependencies }); if (request === previewRequest) outputPreview = result; }
    catch (e) { if (request === previewRequest) previewError = String(e); }
    finally { if (request === previewRequest) previewLoading = false; }
  }
  let workflow: 'python' | 'native' = 'native', workspaceMib = 512;
  let page: 'encoder' | 'decoder' | 'settings' = 'encoder';
  let player: { pausePlayback: () => void } | undefined;
  let roundDown = false, eofPass = false;
  type DependencyKey = 'ffmpeg' | 'ffprobe' | 'python' | 'script' | 'tvcEncoder' | 'tnv3Tool' | 'tacTool' | 'subtitleFont';
  let dependencies: Record<DependencyKey, string> = { ffmpeg: '', ffprobe: '', python: '', script: '', tvcEncoder: '', tnv3Tool: '', tacTool: '', subtitleFont: '' };
  type ToolDiscovery = { key: DependencyKey; automatic: string; candidates: string[] };
  let discovered: ToolDiscovery[] = [], discoveryError = '', discovering = false, discoveryRequest = 0;
  const dependencyFields: { key: DependencyKey; label: string; help: string }[] = [
    { key: 'ffmpeg', label: 'FFmpeg', help: 'Video and audio decoding, both encoders' },
    { key: 'ffprobe', label: 'FFprobe', help: 'Source inspection for the C encoder' },
    { key: 'python', label: 'Python 3', help: 'Runs the C encoder script' },
    { key: 'script', label: 'C encoder script', help: 'Select encode_film.py' },
    { key: 'tvcEncoder', label: 'TVC encoder', help: 'Select tvc_enc (or tvc_enc.exe)' },
    { key: 'tnv3Tool', label: 'TNV3 tool', help: 'Select tnv3 (or tnv3.exe)' },
    { key: 'tacTool', label: 'TAC audio encoder', help: 'Select tac (or tac.exe)' },
    { key: 'subtitleFont', label: 'Subtitle font', help: 'Optional .ttf font for subtitles; blank uses the built-in Roboto' },
  ];
  function saveDependencies() {
    try { localStorage.setItem('tau-encoder-dependencies', JSON.stringify(dependencies)); } catch { /* Keep settings for this session. */ }
    void refreshTools();
    void refreshOutputPreview();
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
  type Progress = { phase: 'probing' | 'video' | 'c-video' | 'audio' | 'subtitles' | 'saving'; frames: number; total_frames_estimate: number | null; remaining_seconds: number | null };
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
      explanations.push('The source container has malformed or duplicate elements. Video or audio data may be skipped or damaged; container warnings alone do not prove an audio decoding fault.');
    }
    if (lower.includes('could not find ref with poc')) {
      explanations.push('The HEVC decoder could not find reference pictures. Affected frames may have visible corruption, missing detail, or discontinuities.');
    }
    if (!explanations.length) explanations.push('FFmpeg reported errors while reading the source. The video or audio may be affected; consult the details below.');
    explanations.push('The app did not repair or verify the affected source data. Check the output, and use a clean source if available.');
    return explanations;
  }
  type ResultInfo = { frames: number; level: number; reencoded: number; video: string; audio: string; warnings: string[]; log: string | null };
  onMount(() => { void invoke<boolean>('experimental_rust_encoder_available').then(value => { rustAvailable = value; }).catch(() => { rustAvailable = false; }); try {
    folder = localStorage.getItem('tau-encoder-output-folder') || '';
    alwaysContinue = localStorage.getItem('tau-encoder-always-continue-source-errors') === 'true';
    keepLog = localStorage.getItem('tau-encoder-keep-log') !== 'false';
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
    if (next === 'settings') { void refreshTools(); void invoke<string>('log_directory').then(d => { logDir = d; }).catch(() => { logDir = ''; }); }
  }
  async function chooseInput() {
    try {
      const file = await open({ multiple: false, directory: false, filters: [{ name: 'Video', extensions: ['mp4', 'mkv', 'mov', 'avi', 'webm', 'm4v', 'mpg', 'mpeg', 'y4m'] }, { name: 'All files', extensions: ['*'] }] });
      if (file) { input = file; name = file.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || 'video'; message = ''; error = ''; warning = ''; void refreshOutputPreview(); void refreshTracks(); }
    } catch (e) { error = String(e); }
  }
  async function chooseFolder() {
    try {
      const dir = await open({ multiple: false, directory: true, defaultPath: folder || undefined });
      if (dir) { folder = dir; try { localStorage.setItem('tau-encoder-output-folder', folder); } catch { /* Keep the current selection. */ } message = ''; error = ''; warning = ''; }
    } catch (e) { error = String(e); }
  }
  async function encode() {
    if (!input || !folder || !name || busy || previewLoading || !validDimensions() || invalidLevel() || (mode === 'c' && workflow === 'native' && (!Number.isInteger(Number(workspaceMib)) || Number(workspaceMib) < 1 || Number(workspaceMib) > 16_384))) return;
    busy = true; error = ''; message = ''; warning = '';
    stopRequested = false;
    progress = { phase: 'probing', frames: 0, total_frames_estimate: null, remaining_seconds: null };
    let unlisten: UnlistenFn | undefined, unlistenSource: UnlistenFn | undefined, unlistenSourceCancelled: UnlistenFn | undefined;
    const jobId = crypto.randomUUID();
    let sourceDecisionCancelled = false;
    try {
      let acceptedSourceErrors = alwaysContinue;
      // The native path reports source errors during decoding. Keep the
      // existing preflight for the Python and experimental Rust paths until
      // their live recovery integration is complete.
      if (!alwaysContinue && !(mode === 'c' && workflow === 'native')) {
        checkingSource = true;
        let check: SourceCheckResult;
        try { check = await invoke<SourceCheckResult>('check_source', { input, audioTrack: Number(audioTrack), dependencies }); }
        finally { checkingSource = false; }
        if (check.status === 'stopped') { message = 'Source check stopped. No output files were created.'; return; }
        if (check.status === 'warning') {
          if (!(await askSourceDecision(check.details))) { message = 'Encoding stopped before output files were created.'; return; }
          acceptedSourceErrors = true;
        }
      }
      unlisten = await listen<Progress>('encode-progress', event => { progress = event.payload; encodeRegistered = true; });
      unlistenSource = await listen<SourceDecisionEvent>('source-error-decision', async event => {
        if (event.payload.job_id !== jobId || sourceDecisionCancelled) return;
        const proceed = await askSourceDecision(event.payload.details);
        if (sourceDecisionCancelled) return;
        try { await invoke('respond_source_error', { jobId, proceed }); } catch (e) { error = String(e); }
      });
      unlistenSourceCancelled = await listen<{ job_id: string }>('source-error-decision-cancelled', event => {
        if (event.payload.job_id !== jobId) return;
        sourceDecisionCancelled = true;
        if (resolveSourceDecision) decideSource(false);
      });
      const width = outputWidth(), height = 720;
      encodeRunning = true;
      const result = await invoke<ResultInfo>('encode', { jobId, input, folder, name: name + suffix, qp: Number(qp), mode, workflow: mode === 'rust' ? null : workflow, workspaceMib: Number(workspaceMib), continueSourceErrors: acceptedSourceErrors, advanced: { keepLog, roundDown, eofPass, audioTrack: Number(audioTrack), subtitleTrack: Number(subtitleTrack) >= 0 ? Number(subtitleTrack) : null, width, height, level: mode === 'rust' ? 0 : Number(selectedLevel) }, dependencies });
      warning = result.warnings.join('\n\n');
      message = `Finished: ${result.frames} frames · level ${result.level}. Created ${result.video}${result.audio ? ' and ' + result.audio : ' (audio is inside the file)'}. Level limits may cause automatic quality reductions.${result.log ? ' Log: ' + result.log : ''}`;
    } catch (e) { const text = String(e); if (text.startsWith('Encoding stopped')) message = text; else error = text; } finally { unlisten?.(); unlistenSource?.(); unlistenSourceCancelled?.(); encodeRunning = false; encodeRegistered = false; stopRequested = false; busy = false; }
  }
</script>
<svelte:head><title>Tau Encoder</title></svelte:head>
<div class="app-shell">
  <nav class="side-nav" aria-label="Main navigation">
    <div class="nav-brand"><img class="mark logo" src="/tau-encoder-logo.svg" alt="" width="40" height="40" /><div><p class="eyebrow">TAU VIDEO TOOLS</p><strong>Tau Encoder</strong></div></div>
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
  <p class="intro">Convert a video into a Tau video file with its audio inside.</p>
  <section aria-label="Video settings">
    <div class="field"><label for="input">Source video <small>MP4, MKV, MOV and more</small></label><div class="pick"><input id="input" value={input} readonly placeholder="Choose a video file" /><button type="button" onclick={chooseInput} disabled={busy}>Browse</button></div></div>
    {#if audioTracks.length > 1}<div class="field"><label for="audioTrack">Audio track <small>This source has {audioTracks.length} audio tracks</small></label><select id="audioTrack" bind:value={audioTrack} disabled={busy}>{#each audioTracks as t}<option value={t.index}>{trackLabel(t)}</option>{/each}</select></div>{/if}
    {#if subtitleTracks.length > 0}<div class="field"><label for="subtitleTrack">Subtitles <small>Text subtitles are drawn into the file; uses the built-in Roboto unless you choose a font in Settings</small></label><select id="subtitleTrack" bind:value={subtitleTrack} disabled={busy}><option value={-1}>No subtitles</option>{#each subtitleTracks as t}<option value={t.index} disabled={!t.text}>{subtitleLabel(t)}</option>{/each}</select></div>{/if}
    <div class="field"><label for="folder">Output folder <small>Choose a folder or card</small></label><div class="pick"><input id="folder" value={folder} readonly placeholder="Choose where to save" /><button type="button" onclick={chooseFolder} disabled={busy}>Browse</button></div></div>
    <div class="field"><label for="name">File name <small>Creates one .tnv3 file with audio inside</small></label><input class="name" id="name" type="text" bind:value={name} disabled={busy} placeholder="Video name" /></div>
    {#if rustAvailable}<div class="field"><label for="mode">Encoder</label><select id="mode" value={mode} onchange={changeMode} disabled={busy}><option value="c">C reference encoder</option><option value="rust">Rust encoder (experimental)</option></select></div>{/if}
    {#if mode === 'c'}
      <div class="field"><label for="workflow">C encoder workflow</label><select id="workflow" bind:value={workflow} onchange={() => void refreshTools()} disabled={busy}><option value="python">Python + C (reference)</option><option value="native">Rust + C (no Python, default)</option></select>
        {#if workflow === 'python'}<p class="workflow-note">Uses Python 3 to coordinate the segmented C video encoder. Requires Python in addition to the existing tools.</p>{:else}<p class="workflow-note">Rust handles orchestration; the same C video encoder still performs compression. Python is not required. This workflow tracks a projected temporary workspace budget; long films may need a higher value. Encoding stops when the projected workspace exceeds this budget, though actual disk use can vary. Video progress updates as segments finish. Custom tools must not launch other processes, because Stop cannot guarantee cleanup of their descendants.</p>
          <label class="workspace-label" for="workspace-mib">Projected workspace budget <small>MiB</small></label><input class="workspace-input" id="workspace-mib" type="number" min="1" max="16384" step="1" bind:value={workspaceMib} disabled={busy} />
        {/if}
      </div>
    {/if}
    <div class="field"><label for="suffix">Output suffix <small>Added to the file name</small></label><input class="name" id="suffix" bind:value={suffix} disabled={busy} /></div>
    <div class="field"><label for="resolution">Output width <small>Pixels</small></label><select id="resolution" value={resolution} onchange={(event) => { resolution = (event.currentTarget as HTMLSelectElement).value; void refreshOutputPreview(); }} disabled={busy}><option value="240">240 pixels</option><option value="360">360 pixels</option><option value="400">400 pixels (default)</option><option value="800">800 pixels</option><option value="custom">Custom</option></select>{#if resolution === 'custom'}<label for="custom-width">Custom width <input id="custom-width" type="number" min="2" max="800" step="2" value={customWidth} onchange={(event) => { customWidth = Number((event.currentTarget as HTMLInputElement).value); void refreshOutputPreview(); }} disabled={busy} /></label>{#if !validDimensions()}<p class="status error">Choose an even width from 2 to 800 pixels.</p>{/if}{/if}<p class="workflow-note">Height follows the source aspect ratio. Very tall video is scaled down if needed to fit the 720-pixel height limit.</p>{#if previewLoading}<p class="workflow-note">Checking output size and level…</p>{:else if outputPreview}<p class="workflow-note">Output: {outputPreview.width} × {outputPreview.height} · lowest compatible level: {outputPreview.minimum_level}.</p>{:else if previewError}<p class="status error">{previewError}</p>{/if}</div>
    {#if mode === 'c'}<div class="field"><label for="level">Codec level</label><select id="level" bind:value={selectedLevel} disabled={busy}><option value={0}>Automatic — lowest compatible level</option><option value={1}>Level 1</option><option value={2}>Level 2</option><option value={3}>Level 3</option></select>{#if invalidLevel()}<p class="status error">This output requires at least level {outputPreview?.minimum_level}. Choose Automatic or a compatible level.</p>{/if}<p class="workflow-note">The encoder selects or validates the level from the actual output. It may increase individual picture QP to stay within that level’s rate limits.</p></div>{/if}
    <div class="field quality"><div><label for="qp">Quality <small>QP</small></label><p>Lower values use more space. Default: 30. QP may rise for individual pictures to satisfy level limits.</p></div><input id="qp" type="number" min="0" max="51" step="1" bind:value={qp} disabled={busy} /></div>
    <button class="primary" type="button" onclick={encode} disabled={busy || previewLoading || !input || !folder || !name.trim() || !validDimensions() || invalidLevel() || qp == null || qp < 0 || qp > 51 || !Number.isInteger(qp) || (mode === 'c' && workflow === 'native' && (!Number.isInteger(Number(workspaceMib)) || Number(workspaceMib) < 1 || Number(workspaceMib) > 16_384))}>{busy ? 'Encoding…' : 'Encode video and audio'}</button>
    {#if busy}
      <div class="progress-area" aria-live="polite">
        <div class="progress-heading"><strong>{checkingSource ? 'Checking source video and audio' : progress.phase === 'probing' ? 'Reading video information' : progress.phase === 'video' ? 'Decoding and encoding video' : progress.phase === 'c-video' ? 'Encoding video with C' : progress.phase === 'audio' ? 'Encoding audio' : progress.phase === 'subtitles' ? 'Drawing subtitles' : 'Saving files'}</strong>
          {#if progress.phase === 'video' && progress.total_frames_estimate && progress.frames < progress.total_frames_estimate}<span>~{Math.min(99, Math.floor(progress.frames / progress.total_frames_estimate * 100))}%</span>{/if}
        </div>
        {#if progress.phase === 'video' && progress.total_frames_estimate && progress.frames < progress.total_frames_estimate}
          <progress aria-label="Estimated video progress" value={progress.frames} max={progress.total_frames_estimate}></progress>
        {:else}
          <progress aria-label="Encoding progress"></progress>
        {/if}
        <p class="progress-detail">{checkingSource ? 'Checking can take as long as decoding the source. Encoding starts after the check.' : progressDetail(progress)}</p>
        {#if checkingSource}<button type="button" onclick={stopChecking}>Stop source check</button>{/if}
        {#if encodeRunning && encodeRegistered && progress.phase !== 'saving'}<button type="button" onclick={stopAfterPhase} disabled={stopRequested}>{stopRequested ? 'Stop requested' : mode === 'c' && workflow === 'native' ? 'Stop encoding' : 'Stop after current phase'}</button>{#if stopRequested}<p class="progress-detail">{mode === 'c' && workflow === 'native' ? 'Stopping and waiting for running tools to exit. No final files will be created.' : 'Waiting for the current video or audio phase to finish. No final files will be created.'}</p>{/if}{/if}
      </div>
    {/if}
    {#if message}<p class="status success" role="status">{message}</p>{/if}
    {#if warning}<div class="status warning-summary" role="alert"><strong>Output created with source warnings</strong>{#each warningExplanations(warning) as explanation}<p>{explanation}</p>{/each}<details><summary>Show original decoder messages</summary><pre class="warning">{warning}</pre></details></div>{/if}
    {#if error}<p class="status error" role="alert">{error}</p>{/if}
  </section>
  <aside><strong>Early preview</strong><p>Output is 24 fps with 48 kHz stereo TAC audio stored inside the .tnv3 file. Some videos may fail or require lower quality. Time estimates cover video processing only. The Python workflow stops after the current phase; the native workflow requests that its running tools stop.</p></aside>
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
    </details>
    <section class="recovery-settings" aria-label="Encode log"><h2>Encode log</h2><label><input type="checkbox" bind:checked={keepLog} onchange={saveKeepLog} /> Keep a detailed log of every encode</label><p class="help">One file per encode (JSON lines: settings, tools, source, progress every second, phase times, warnings, result or error) for analysing speed and problems. Logs are never deleted automatically.</p><div class="player-actions"><button type="button" onclick={openLogFolder}>Open log folder</button></div>{#if logDir}<p class="help detected-path">{logDir}</p>{/if}{#if logError}<p class="status error" role="alert">{logError}</p>{/if}</section>
    <section class="recovery-settings" aria-label="Source error handling"><h2>Source error handling</h2><label><input type="checkbox" checked={alwaysContinue} onchange={(event) => { alwaysContinue = (event.currentTarget as HTMLInputElement).checked; saveRecoveryPreference(); }} disabled={busy} /> Always continue when source errors are found</label><p class="help">When off, the native C workflow asks when it finds a source problem. The Python workflow checks the source before encoding, which can take as long as decoding. When on, the app continues without asking and reports errors with the result.</p></section>
  {/if}
  <div class="decoder-player" hidden={page !== 'decoder'}><Player bind:this={player} /></div>
</main>
</div>
{#if resolveSourceDecision}<dialog use:openModal oncancel={(event) => { event.preventDefault(); decideSource(false); }} class="source-dialog" aria-labelledby="source-dialog-title" aria-describedby="source-dialog-description"><h2 id="source-dialog-title">Source problem detected</h2><p id="source-dialog-description">FFmpeg reported a problem reading the source. Continuing may still fail.</p>{#each warningExplanations(sourceIssue) as explanation}<p>{explanation}</p>{/each}<details><summary>Show the reported error</summary><pre>{sourceIssue}</pre></details><label><input type="checkbox" bind:checked={rememberDecision} /> Always continue for source errors</label><div class="dialog-actions"><button type="button" use:focusDialogButton onclick={() => decideSource(false)}>Stop</button><button type="button" class="primary" onclick={() => decideSource(true)}>Continue encoding</button></div></dialog>{/if}

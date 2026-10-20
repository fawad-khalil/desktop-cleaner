<script lang="ts">
  import { downloadDir } from "@tauri-apps/api/path";
  import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
  import {
    api, cleanupApi, leftoverApi,
    type CacheInfo, type CleanResult, type Leftover, type Preview, type Target,
  } from "../api";
  import { ago, bytes } from "../format";

  let caches = $state<CacheInfo[] | null>(null);
  let leftovers = $state<Leftover[] | null>(null);
  /** Leftover paths the user ticked. Kept separate so the confirmation can call them out. */
  let pickedLeftovers = $state<Set<string>>(new Set());
  let copied = $state("");
  let picked = $state<Set<string>>(new Set());
  /** Only clean cache items unused for this many days (0 = everything). */
  let cacheAge = $state(0);
  let oldEnabled = $state(false);
  let oldPath = $state("");
  let minAge = $state(180);
  let minSize = $state(10 * 1024 * 1024);

  let preview = $state<Preview | null>(null);
  let selected = $state<Set<string>>(new Set());
  let busy = $state("");
  let error = $state("");
  let result = $state<CleanResult | null>(null);
  let showProtected = $state(false);

  $effect(() => {
    api.listCaches().then((c) => (caches = c.filter((x) => x.size > 0))).catch((e) => (error = String(e)));
    leftoverApi.find().then((l) => (leftovers = l)).catch((e) => (error = String(e)));
    downloadDir().then((d) => (oldPath ||= d)).catch(() => {});
  });

  function toggle(set: Set<string>, key: string): Set<string> {
    const s = new Set(set);
    if (s.has(key)) s.delete(key);
    else s.add(key);
    return s;
  }

  /** Sets an `indeterminate` checkbox state (no attribute exists for it). */
  function indeterminate(node: HTMLInputElement, on: boolean) {
    node.indeterminate = on;
    return { update: (v: boolean) => (node.indeterminate = v) };
  }

  const isTemp = (c: CacheInfo) => c.group === "System temp" || c.group === "Logs";
  const sections = $derived([
    { title: "Caches", blurb: "Browser, app and developer-tool caches. Apps rebuild them when needed.", items: (caches ?? []).filter((c) => !isTemp(c)) },
    { title: "Temporary files & logs", blurb: "Temp folders, crash reports and logs that are safe to clear.", items: (caches ?? []).filter(isTemp) },
  ]);

  type TriState = "all" | "some" | "none";
  function groupState(items: CacheInfo[]): TriState {
    const n = items.filter((c) => picked.has(c.path)).length;
    return n === 0 ? "none" : n === items.length ? "all" : "some";
  }
  const allState = $derived.by((): TriState => {
    const items = caches ?? [];
    const n = items.filter((c) => picked.has(c.path)).length + (oldEnabled ? 1 : 0);
    return n === 0 ? "none" : n === items.length + 1 ? "all" : "some";
  });

  function setPaths(paths: string[], on: boolean) {
    const s = new Set(picked);
    for (const p of paths) on ? s.add(p) : s.delete(p);
    picked = s;
  }
  function selectAll(on: boolean) {
    setPaths((caches ?? []).map((c) => c.path), on);
    oldEnabled = on;
  }

  const anySelected = $derived(picked.size > 0 || pickedLeftovers.size > 0 || (oldEnabled && !!oldPath));
  const selectionSummary = $derived.by(() => {
    const parts: string[] = [];
    const sel = (caches ?? []).filter((c) => picked.has(c.path));
    if (sel.length) parts.push(`${sel.length} cache/temp location${sel.length > 1 ? "s" : ""} (up to ${bytes(sel.reduce((a, c) => a + c.size, 0))})`);
    if (oldEnabled && oldPath) parts.push("old files");
    if (pickedLeftovers.size) parts.push(`${pickedLeftovers.size} leftover item(s)`);
    return parts.join(" + ");
  });

  const trashName = navigator.userAgent.includes("Windows") ? "Recycle Bin" : "Trash";
  let previewEl = $state<HTMLElement | null>(null);
  $effect(() => {
    if (preview && previewEl) previewEl.scrollIntoView({ behavior: "smooth", block: "start" });
  });

  function togglePaths(paths: string[], on: boolean) {
    const s = new Set(pickedLeftovers);
    for (const p of paths) on ? s.add(p) : s.delete(p);
    pickedLeftovers = s;
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    copied = text;
    setTimeout(() => (copied = ""), 1500);
  }

  async function openTool() {
    try {
      await leftoverApi.openSystemCleanup();
    } catch (e) {
      error = String(e);
    }
  }

  async function browse() {
    const dir = await openDialog({ directory: true, defaultPath: oldPath || undefined });
    if (typeof dir === "string") oldPath = dir;
  }

  async function runPreview() {
    const targets: Target[] = [
      ...[...picked].map((path): Target => ({ kind: "contents", path, ...(cacheAge ? { min_age_days: cacheAge } : {}) })),
      ...[...pickedLeftovers].map((path): Target => ({ kind: "contents", path })),
    ];
    if (oldEnabled && oldPath)
      targets.push({ kind: "old_files", path: oldPath, min_age_days: minAge, min_size: minSize });
    if (!targets.length) return;
    busy = "Building preview (nothing is deleted)…";
    error = "";
    result = null;
    try {
      preview = await cleanupApi.preview(targets);
      selected = new Set(preview.items.map((i) => i.path));
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }

  const selectedSize = $derived(
    (preview?.items ?? []).filter((i) => selected.has(i.path)).reduce((a, i) => a + i.size, 0),
  );

  async function clean() {
    const paths = [...selected];
    if (!paths.length) return;
    const ok = await ask(
      `Move ${paths.length} item(s), ${bytes(selectedSize)}, to the ${trashName}?\n\nYou can restore them from there until it is emptied.`,
      { title: "Confirm cleanup", kind: "warning", okLabel: `Move to ${trashName}`, cancelLabel: "Cancel" },
    );
    if (!ok) return;
    busy = `Moving items to the ${trashName}…`;
    try {
      result = await cleanupApi.execute(paths);
      preview = null;
      selected = new Set();
      pickedLeftovers = new Set();
      api.listCaches().then((c) => (caches = c.filter((x) => x.size > 0)));
      leftoverApi.find().then((l) => (leftovers = l));
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }
</script>

<header>
  <h1>Cleanup</h1>
  <p class="muted">
    Tick what you'd like to clean, choose Show selected to see exactly which files that covers, then move the ones
    you pick to the {trashName}.
    System files are always excluded.
  </p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if result}
  <div class="card rec">
    <strong>Moved {result.moved} item(s) to the {trashName}, freeing {bytes(result.freed)}.</strong>
    {#if result.failed.length}
      <p class="muted">{result.failed.length} item(s) could not be moved (often because they are in use):</p>
      <ul class="small">
        {#each result.failed.slice(0, 20) as f}<li><code class="path">{f.path}</code>: {f.reason}</li>{/each}
      </ul>
    {/if}
  </div>
{/if}

<div class="card">
  <div class="row head">
    <label class="check master">
      <input type="checkbox" checked={allState === "all"} use:indeterminate={allState === "some"}
        onchange={(e) => selectAll(e.currentTarget.checked)} disabled={!caches} />
      <h3>Caches, temporary files &amp; old files</h3>
    </label>
    <span class="muted">Tick what you'd like to clean up, then choose <strong>Show selected</strong> to review the exact files.</span>
  </div>

  <div class="toolbar">
    <span class="muted">Only include cache and temporary items not used for</span>
    <select bind:value={cacheAge}>
      <option value={0}>any time (everything)</option>
      <option value={7}>7 days</option>
      <option value={30}>30 days</option>
      <option value={90}>3 months</option>
      <option value={180}>6 months</option>
      <option value={365}>1 year</option>
    </select>
  </div>

  {#if !caches}
    <p class="muted">Measuring caches…</p>
  {:else}
    {#each sections as sec}
      {#if sec.items.length}
        {@const st = groupState(sec.items)}
        <div class="sub">
          <label class="check subhead">
            <input type="checkbox" checked={st === "all"} use:indeterminate={st === "some"}
              onchange={(e) => setPaths(sec.items.map((c) => c.path), e.currentTarget.checked)} />
            <strong class="grow">{sec.title}</strong>
            <span class="muted">{bytes(sec.items.reduce((a, c) => a + c.size, 0))}</span>
          </label>
          <p class="muted blurb">{sec.blurb}</p>
          {#each sec.items as c}
            <label class="check item">
              <input type="checkbox" checked={picked.has(c.path)} onchange={() => (picked = toggle(picked, c.path))} />
              <span class="grow" title={c.path}>{c.name}</span>
              <span class="muted">{bytes(c.size)}</span>
            </label>
          {/each}
        </div>
      {/if}
    {/each}
    {#if cacheAge}
      <p class="note">Sizes are the whole folder; only the items nothing has used for {cacheAge} days or more will be listed when you choose Show selected.</p>
    {/if}
  {/if}

  <div class="sub">
    <label class="check subhead">
      <input type="checkbox" bind:checked={oldEnabled} />
      <strong class="grow">Old files</strong>
    </label>
    <p class="muted blurb">Files in a folder of your choice that you haven't opened or changed in a long time.</p>
    <div class="toolbar item">
      <input class="grow" bind:value={oldPath} placeholder="Folder to search" onfocus={() => (oldEnabled = true)} />
      <button onclick={browse}>Browse…</button>
    </div>
    <div class="toolbar item">
      <span class="muted">Not used for</span>
      <select bind:value={minAge} onchange={() => (oldEnabled = true)}>
        <option value={30}>30 days</option>
        <option value={90}>3 months</option>
        <option value={180}>6 months</option>
        <option value={365}>1 year</option>
        <option value={730}>2 years</option>
      </select>
      <span class="muted">and at least</span>
      <select bind:value={minSize} onchange={() => (oldEnabled = true)}>
        <option value={0}>any size</option>
        <option value={1024 * 1024}>1 MB</option>
        <option value={10 * 1024 * 1024}>10 MB</option>
        <option value={100 * 1024 * 1024}>100 MB</option>
        <option value={1024 * 1024 * 1024}>1 GB</option>
      </select>
    </div>
    <p class="note item">"Used" means the later of last opened and last changed.</p>
  </div>

  <div class="toolbar actions">
    <button class="primary" onclick={runPreview} disabled={!!busy || !anySelected}>Show selected</button>
    <span class="muted">
      {#if busy}{busy}{:else if anySelected}{selectionSummary}. Nothing is deleted yet; you'll review the files first.{:else}Nothing selected yet.{/if}
    </span>
  </div>
</div>

<div class="card">
  <h3>Leftovers from updates and installers <span class="badge">confirm before delete</span></h3>
  {#if !leftovers}
    <p class="muted">Looking for update leftovers…</p>
  {:else if !leftovers.length}
    <p class="muted">No update leftovers found.</p>
  {:else}
    {#each leftovers as l}
      <div class="leftover">
        <div class="lhead">
          {#if l.action === "trash"}
            <input
              type="checkbox"
              checked={l.paths.every((p) => pickedLeftovers.has(p))}
              onchange={(e) => togglePaths(l.paths, e.currentTarget.checked)}
            />
          {/if}
          <strong class="grow">{l.title}</strong>
          <span class="muted">{bytes(l.size)}</span>
        </div>
        <p class="muted small-p">{l.description}</p>
        <div class="small-p">{#each l.paths as p}<code class="path">{p}</code> {/each}</div>
        {#if l.action === "system_tool"}
          <button onclick={openTool}>Open Disk Cleanup…</button>
          <span class="note">In Disk Cleanup, choose "Clean up system files" and tick the matching entry.</span>
        {:else if l.action === "command" && l.command}
          <div class="toolbar">
            <code class="cmd">{l.command}</code>
            <button onclick={() => copy(l.command!)}>{copied === l.command ? "Copied" : "Copy"}</button>
          </div>
          <span class="note">Run this in a terminal yourself. DesktopCleaner never runs commands as root.</span>
        {:else if l.action === "manual"}
          <span class="note">This lives in a protected system area, so DesktopCleaner won't delete it. Remove it in Finder if you don't need it.</span>
        {/if}
      </div>
    {/each}
  {/if}
</div>

{#if pickedLeftovers.size}
  <div class="toolbar">
    <button class="primary" onclick={runPreview} disabled={!!busy}>Show selected</button>
    <span class="muted">{pickedLeftovers.size} leftover item(s) ticked{picked.size || oldEnabled ? ", plus your choices above" : ""}.</span>
  </div>
{/if}

{#if preview}
  <div class="card" bind:this={previewEl}>
    <h3>Review: these will be moved to the {trashName}</h3>
    <div class="toolbar">
      <strong>{preview.items.length} items · {bytes(preview.total)}</strong>
      {#if preview.truncated}<span class="muted">(showing the largest {preview.items.length})</span>{/if}
      <button onclick={() => (selected = new Set(preview!.items.map((i) => i.path)))}>Select all</button>
      <button onclick={() => (selected = new Set())}>Select none</button>
      <span class="grow"></span>
      <button class="danger" disabled={!selected.size || !!busy} onclick={clean}>
        Move {selected.size} selected ({bytes(selectedSize)}) to {trashName}
      </button>
    </div>
    {#if preview.protected_count}
      <p class="muted">
        🛡 {preview.protected_count} protected item(s) were excluded.
        <button class="link" onclick={() => (showProtected = !showProtected)}>{showProtected ? "Hide" : "Show"}</button>
      </p>
      {#if showProtected}
        <ul class="small">
          {#each preview.protected as r}<li><code class="path">{r.path}</code>: {r.reason}</li>{/each}
        </ul>
      {/if}
    {/if}
    <table>
      <thead><tr><th></th><th>Item</th><th class="num">Last used</th><th class="num">Size</th></tr></thead>
      <tbody>
        {#each preview.items.slice(0, 1000) as i (i.path)}
          <tr>
            <td><input type="checkbox" checked={selected.has(i.path)} onchange={() => (selected = toggle(selected, i.path))} /></td>
            <td><code class="path">{i.is_dir ? "📁" : "📄"} {i.path}</code></td>
            <td class="num">{ago(i.last_used)}</td>
            <td class="num">{bytes(i.size)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if preview.items.length > 1000}<p class="muted">…and {preview.items.length - 1000} more (all included in the selection).</p>{/if}
  </div>
{/if}

<style>
  h3 { margin: 0 0 10px; font-size: 14px; }
  .check { display: flex; gap: 8px; align-items: center; padding: 4px 0; cursor: pointer; }
  .head { align-items: center; flex-wrap: wrap; }
  .master h3 { margin: 0; }
  .sub { border-top: 1px solid var(--border); padding: 10px 0 6px; }
  .subhead { padding: 2px 0; }
  .blurb { margin: 0 0 4px 26px; font-size: 12px; }
  .item { margin-left: 26px; }
  .actions { margin: 12px 0 0; border-top: 1px solid var(--border); padding-top: 12px; }
  .grow { flex: 1; }
  .small { font-size: 12px; margin: 6px 0; padding-left: 18px; }
  .badge { font-size: 11px; font-weight: 600; color: #fff; background: var(--warn); border-radius: 6px; padding: 2px 6px; margin-left: 6px; }
  .leftover { border-top: 1px solid var(--border); padding: 10px 0; }
  .leftover:first-of-type { border-top: none; }
  .lhead { display: flex; gap: 8px; align-items: center; }
  .small-p { margin: 4px 0 6px; font-size: 12px; }
  .cmd { background: var(--bg); padding: 6px 8px; border-radius: 6px; flex: 1; word-break: break-all; }
</style>

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
      `Move ${paths.length} item(s), ${bytes(selectedSize)}, to the Recycle Bin / Trash?\n\nYou can restore them from there until it is emptied.`,
      { title: "Confirm cleanup", kind: "warning", okLabel: "Move to Recycle Bin", cancelLabel: "Cancel" },
    );
    if (!ok) return;
    busy = "Moving items to the Recycle Bin…";
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
    Pick what to clean, preview exactly what would go, then move your selection to the Recycle Bin / Trash.
    System files are always excluded.
  </p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if result}
  <div class="card rec">
    <strong>Moved {result.moved} item(s) to the Recycle Bin, freeing {bytes(result.freed)}.</strong>
    {#if result.failed.length}
      <p class="muted">{result.failed.length} item(s) could not be moved (often because they are in use):</p>
      <ul class="small">
        {#each result.failed.slice(0, 20) as f}<li><code class="path">{f.path}</code>: {f.reason}</li>{/each}
      </ul>
    {/if}
  </div>
{/if}

<div class="grid two">
  <div class="card">
    <h3>Caches &amp; temp</h3>
    <div class="toolbar">
      <span class="muted">Not used for</span>
      <select bind:value={cacheAge}>
        <option value={0}>any time (clean everything)</option>
        <option value={7}>7 days</option>
        <option value={30}>30 days</option>
        <option value={90}>3 months</option>
        <option value={180}>6 months</option>
        <option value={365}>1 year</option>
      </select>
    </div>
    {#if caches}
      {#each caches as c}
        <label class="check">
          <input type="checkbox" checked={picked.has(c.path)} onchange={() => (picked = toggle(picked, c.path))} />
          <span class="grow">{c.name}</span>
          <span class="muted">{bytes(c.size)}</span>
        </label>
      {/each}
    {:else}
      <p class="muted">Measuring caches…</p>
    {/if}
    {#if cacheAge}
      <p class="note">Only items in the ticked caches that nothing has used for {cacheAge} days or more are included; sizes above are the whole cache. The preview shows exactly what qualifies.</p>
    {/if}
  </div>

  <div class="card">
    <h3>Old files</h3>
    <label class="check">
      <input type="checkbox" bind:checked={oldEnabled} />
      <span>Find files not used recently</span>
    </label>
    <div class="toolbar">
      <input class="grow" bind:value={oldPath} placeholder="Folder to search" />
      <button onclick={browse}>Browse…</button>
    </div>
    <div class="toolbar">
      <span class="muted">Not used for</span>
      <select bind:value={minAge}>
        <option value={30}>30 days</option>
        <option value={90}>3 months</option>
        <option value={180}>6 months</option>
        <option value={365}>1 year</option>
        <option value={730}>2 years</option>
      </select>
      <span class="muted">and at least</span>
      <select bind:value={minSize}>
        <option value={0}>any size</option>
        <option value={1024 * 1024}>1 MB</option>
        <option value={10 * 1024 * 1024}>10 MB</option>
        <option value={100 * 1024 * 1024}>100 MB</option>
        <option value={1024 * 1024 * 1024}>1 GB</option>
      </select>
    </div>
    <p class="note">"Used" means the later of last access and last modification.</p>
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

<div class="toolbar">
  <button class="primary" onclick={runPreview} disabled={!!busy || (!picked.size && !pickedLeftovers.size && !oldEnabled)}>
    Preview (dry run)
  </button>
  {#if busy}<span class="muted">{busy}</span>{/if}
</div>

{#if preview}
  <div class="card">
    <div class="toolbar">
      <strong>{preview.items.length} items · {bytes(preview.total)}</strong>
      {#if preview.truncated}<span class="muted">(showing the largest {preview.items.length})</span>{/if}
      <button onclick={() => (selected = new Set(preview!.items.map((i) => i.path)))}>Select all</button>
      <button onclick={() => (selected = new Set())}>Select none</button>
      <span class="grow"></span>
      <button class="danger" disabled={!selected.size || !!busy} onclick={clean}>
        Move {selected.size} selected ({bytes(selectedSize)}) to Recycle Bin
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
  .grow { flex: 1; }
  .small { font-size: 12px; margin: 6px 0; padding-left: 18px; }
  .badge { font-size: 11px; font-weight: 600; color: #fff; background: var(--warn); border-radius: 6px; padding: 2px 6px; margin-left: 6px; }
  .leftover { border-top: 1px solid var(--border); padding: 10px 0; }
  .leftover:first-of-type { border-top: none; }
  .lhead { display: flex; gap: 8px; align-items: center; }
  .small-p { margin: 4px 0 6px; font-size: 12px; }
  .cmd { background: var(--bg); padding: 6px 8px; border-radius: 6px; flex: 1; word-break: break-all; }
</style>

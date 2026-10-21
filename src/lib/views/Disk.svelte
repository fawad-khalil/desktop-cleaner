<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { api, type DriveInfo, type Entry, type FileItem, type Refreshed, type ScanResult } from "../api";
  import { ago, bytes, freeColor, pct, since, systemColor, typeColors } from "../format";
  import Pie, { type Slice } from "../Pie.svelte";
  import StorageBar, { type Segment } from "../StorageBar.svelte";
  import Sunburst from "../Sunburst.svelte";
  import SearchBox from "../SearchBox.svelte";
  import { emptySearch, forBackend, matcher } from "../search";

  const isMac = navigator.userAgent.includes("Mac");

  let drives = $state<DriveInfo[]>([]);
  let error = $state("");
  /** Whole-drive scans by mount point; they colour the storage bars. */
  let driveScans = $state<Record<string, ScanResult>>({});
  let scan = $state<ScanResult | null>(null);
  let scanning = $state("");
  let progress = $state(0);
  let autoStarted = false;
  /** Root of the index being brought up to date in the background. */
  let refreshing = $state("");
  let lastMethod = $state<Refreshed["method"] | null>(null);
  /** Ticks so "Updated … ago" stays current. */
  let clock = $state(Date.now());

  let filesTitle = $state("");
  /** Which files the panel lists, so a search can re-run it. */
  let filesQuery = $state<{ category: string | null; age: number | null }>({ category: null, age: null });
  /** Search over all files below the folder (backend, Rust regex). */
  let fileSearch = $state(emptySearch());
  let fileSearchError = $state("");
  /** Filter for this folder's table. */
  let entrySearch = $state(emptySearch());
  let files = $state<FileItem[] | null>(null);
  let filesLoading = $state(false);

  const ageColors = ["#34c38f", "#8bc34a", "#f7b924", "#ff9f43", "#e5484d"];

  $effect(() => {
    api.listDrives().then((d) => {
      drives = d;
      // Start on the system drive so the breakdown is there without a click.
      const sys = d.find((x) => x.mount_point === "/" || /^c:\\?$/i.test(x.mount_point)) ?? d[0];
      if (sys && !autoStarted) {
        autoStarted = true;
        open(sys.mount_point);
      }
    }).catch((e) => (error = String(e)));
  });

  $effect(() => {
    const id = setInterval(() => (clock = Date.now()), 30_000);
    // The live watcher changed an index: re-show what's on screen (from memory).
    const un = listen<string>("index-changed", (e) => reload(e.payload));
    return () => {
      clearInterval(id);
      un.then((f) => f());
    };
  });

  /** Re-reads the current view and drive bars for `root` from the index. */
  async function reload(root: string) {
    try {
      if (drives.some((d) => d.mount_point === root)) driveScans[root] = await api.scanDir(root);
      if (scan && scan.root === root && !scanning) scan = await api.scanDir(scan.path);
    } catch {}
  }

  /** Updates the saved index in the background, then re-shows the view. */
  async function refreshIndex(root: string) {
    if (refreshing) return;
    refreshing = root;
    try {
      const r = await api.refreshIndex(root);
      lastMethod = r.method;
      await reload(root);
    } catch (e) {
      error = String(e);
    } finally {
      refreshing = "";
    }
  }

  $effect(() => {
    if (!scanning && !refreshing) return;
    const id = setInterval(() => api.scanProgress().then((n) => (progress = n)).catch(() => {}), 500);
    return () => clearInterval(id);
  });

  async function open(path: string, fresh = false) {
    files = null;
    filesTitle = "";
    error = "";
    scanning = path;
    progress = 0;
    try {
      const res = await api.scanDir(path, fresh);
      if (drives.some((d) => d.mount_point === res.path)) driveScans[res.path] = res;
      scan = res;
      // Shown from the saved index: bring it up to date in the background.
      if (!fresh && Date.now() / 1000 - res.updated_at > 60) refreshIndex(res.root);
    } catch (e) {
      error = String(e);
    } finally {
      scanning = "";
    }
  }

  const sep = $derived(scan?.path.includes("\\") ? "\\" : "/");

  /** Cumulative path segments for the breadcrumb. */
  const crumbs = $derived.by(() => {
    if (!scan) return [];
    const parts = scan.path.split(sep).filter(Boolean);
    const out: { label: string; path: string }[] = [];
    let acc = sep === "/" ? "/" : "";
    if (sep === "/") out.push({ label: driveFor("/")?.name || "/", path: "/" });
    for (const part of parts) {
      acc = sep === "/" ? (acc.endsWith("/") ? acc + part : acc + "/" + part) : acc ? (acc.endsWith("\\") ? acc + part : acc + "\\" + part) : part + "\\";
      out.push({ label: part, path: acc });
    }
    return out;
  });

  function driveFor(mount: string) {
    return drives.find((d) => d.mount_point === mount);
  }

  const used = (d: DriveInfo) => d.total - d.available;

  /** Used space we couldn't attribute to files: OS data, other users,
   * protected folders, snapshots… */
  function hidden(d: DriveInfo, s: ScanResult) {
    return Math.max(0, used(d) - s.total);
  }

  function segments(d: DriveInfo): Segment[] {
    const s = driveScans[d.mount_point];
    const free: Segment = { label: "Free", value: d.available, color: freeColor };
    if (!s) return [{ label: "Used", value: used(d), color: "var(--accent)" }, free];
    // Hard links and clones can make files add up to more than is used.
    const k = s.total > used(d) ? used(d) / s.total : 1;
    const segs: Segment[] = s.types.map((t) => ({ label: t.category, value: t.size * k, color: typeColors[t.category] ?? "#7f8c9a" }));
    segs.push({ label: "System & other", value: hidden(d, s), color: systemColor, hint: hiddenHint });
    segs.push(free);
    return segs;
  }

  const methodText = { events: "from the system's change history", mtime: "by checking which folders changed", full: "by a full rescan" };

  const hiddenHint = isMac
    ? "macOS system data, swap, snapshots and folders DesktopCleaner isn't allowed to read"
    : "System data and folders DesktopCleaner isn't allowed to read";

  const extra = $derived.by(() => {
    if (!scan) return null;
    const d = driveFor(scan.path);
    if (!d) return null;
    return { label: "System & other", value: hidden(d, scan), color: systemColor, hint: hiddenHint };
  });

  async function showFiles(title: string, category: string | null, age: number | null) {
    if (!scan) return;
    filesTitle = title;
    filesQuery = { category, age };
    filesLoading = true;
    files = null;
    try {
      files = await api.largestFiles(scan.path, category, age, 100, forBackend(fileSearch));
    } catch (e) {
      error = String(e);
    } finally {
      filesLoading = false;
    }
  }

  // Re-run the file list when the search changes (debounced); the backend
  // reports invalid patterns.
  $effect(() => {
    const f = { ...fileSearch };
    const t = setTimeout(async () => {
      fileSearchError = f.pattern ? ((await api.checkSearch(f).catch(() => null)) ?? "") : "";
      if (!fileSearchError && filesTitle) showFiles(filesTitle, filesQuery.category, filesQuery.age);
    }, 400);
    return () => clearTimeout(t);
  });

  function findFiles() {
    if (fileSearch.pattern && !fileSearchError) showFiles(`Files matching “${fileSearch.pattern}”`, null, null);
  }

  const entryMatch = $derived(matcher(entrySearch));
  const shownEntries = $derived((scan?.entries ?? []).filter((e) => !entryMatch.test || entryMatch.test(e.path)));

  const typeSlices = $derived<Slice[]>(
    (scan?.types ?? []).map((t) => ({ label: t.category, value: t.size, key: t.category, color: typeColors[t.category] })),
  );
  const ageSlices = $derived<Slice[]>(
    (scan?.ages ?? []).map((t, i) => ({ label: t.category, value: t.size, key: String(i), color: ageColors[i] })),
  );
  const oldShare = $derived(
    scan ? (scan.ages.slice(2).reduce((a, b) => a + b.size, 0) / (scan.total || 1)) * 100 : 0,
  );
  const smaller = $derived(scan ? Math.max(0, scan.total - scan.entries.reduce((a, e) => a + e.size, 0)) : 0);
  const folderName = $derived(crumbs.at(-1)?.label ?? "");
</script>

<header>
  <h1>Disk usage</h1>
  <p class="muted">See what fills each drive, then click any part of the chart to dig deeper, all the way down to single files.</p>
</header>

{#if error}<p class="error">{error}</p>{/if}

<div class="grid drives">
  {#each drives as d}
    {@const s = driveScans[d.mount_point]}
    <div class="card" class:current={scan && crumbs[0]?.path === d.mount_point}>
      <div class="row">
        <strong>{d.name || d.mount_point}</strong>
        <span class="muted">{d.mount_point} · {d.file_system}{d.removable ? " · removable" : ""}</span>
      </div>
      <div class="row usage">
        <span><strong>{bytes(used(d))}</strong> <span class="muted">used of {bytes(d.total)}</span></span>
        <span class="muted">{bytes(d.available)} free ({pct(d.available, d.total)})</span>
      </div>
      <StorageBar segments={segments(d)} total={d.total} />
      <div class="actions">
        {#if scanning === d.mount_point}
          <span class="muted">Scanning… {progress.toLocaleString()} files so far</span>
        {:else if s}
          <button onclick={() => open(d.mount_point)} disabled={!!scanning}>Explore</button>
          <span class="muted small">{refreshing === d.mount_point ? "Updating…" : `Updated ${since(s.updated_at, clock)}`}</span>
        {:else}
          <button class="primary" disabled={!!scanning} onclick={() => open(d.mount_point)}>Analyze</button>
          <span class="muted">to break down used space</span>
        {/if}
      </div>
    </div>
  {/each}
</div>

{#if scanning && !driveFor(scanning)}
  <p class="muted">Scanning {scanning}… {progress.toLocaleString()} files so far</p>
{:else if scanning && !scan}
  <p class="muted">The first scan reads every file on the drive and can take a few minutes. It only reads; nothing is changed.</p>
{/if}

{#if scan}
  <div class="toolbar">
    <div class="crumbs">
      {#each crumbs as c, i}
        {#if i > 0}<span class="muted">›</span>{/if}
        <button class="link" disabled={!!scanning} onclick={() => open(c.path)}>{c.label}</button>
      {/each}
    </div>
    <span class="grow"></span>
    <span class="muted">{bytes(scan.total)} in {scan.files.toLocaleString()} files</span>
    <span class="muted" title={`Last full scan ${since(scan.full_scan_at, clock)}${lastMethod ? `; last update ${methodText[lastMethod]}` : ""}`}>
      {#if refreshing === scan.root}Updating… {progress ? `(${progress.toLocaleString()} files checked)` : ""}{:else}Updated {since(scan.updated_at, clock)}{/if}
    </span>
    {#if scan.path !== scan.root}
      <button onclick={() => open(scan!.path, true)} disabled={!!scanning || !!refreshing} title="Read just this folder from disk again">Rescan folder</button>
    {/if}
    <button onclick={() => open(scan!.root, true)} disabled={!!scanning || !!refreshing} title="Read the whole drive again from scratch">Full rescan</button>
  </div>

  {#if isMac && scan.skipped && driveFor(scan.path)}
    <div class="card hint">
      macOS kept DesktopCleaner out of {scan.skipped.toLocaleString()} protected folders (Mail, Messages, other apps' data…), so they
      count as <em>System &amp; other</em>. For a complete picture, allow Full Disk Access and rescan.
      <button class="link" onclick={() => api.openFullDiskAccess()}>Open Privacy settings</button>
    </div>
  {/if}

  <div class="card">
    <h3>Used space in {folderName}</h3>
    <Sunburst
      entries={scan.entries}
      total={scan.total}
      name={folderName}
      {extra}
      canUp={crumbs.length > 1}
      onup={() => open(crumbs[crumbs.length - 2].path)}
      onselect={(e: Entry) => open(e.path)}
    />
    <p class="note">Inner ring: what's directly in this folder. Outer ring: what's inside each of those. Click a folder to open it; click the middle to go back up.</p>
  </div>

  <div class="grid two">
    <div class="card">
      <Pie title="By file type" slices={typeSlices} max={12}
        onselect={(s) => s.key && showFiles(`Largest files: ${s.label}`, s.key, null)} />
    </div>
    <div class="card">
      <Pie title="By last used" slices={ageSlices}
        onselect={(s) => showFiles(`Largest files ${s.label.toLowerCase()}`, null, Number(s.key))} />
      {#if oldShare > 25}
        <p class="note">{oldShare.toFixed(0)}% of this folder hasn't been used in over 6 months. The Cleanup tab can find and remove old files.</p>
      {/if}
    </div>
  </div>

  <div class="card">
    <div class="toolbar find">
      <strong>Find files in {folderName}</strong>
      <SearchBox bind:filter={fileSearch} placeholder="Name or path, e.g. .mov or \.(iso|dmg)$ with .*" error={fileSearchError} />
      <button onclick={findFiles} disabled={!fileSearch.pattern || !!fileSearchError}>Find largest matches</button>
    </div>
    {#if !filesTitle}
      <p class="note">Searches every file below this folder. Clicking a slice in the type or last-used chart also lists files, narrowed by this search.</p>
    {/if}
  {#if filesTitle}
      <div class="row">
        <strong>{filesTitle}{fileSearch.pattern && !filesTitle.startsWith("Files matching") ? ` matching “${fileSearch.pattern}”` : ""}</strong>
        <button class="link" onclick={() => { filesTitle = ""; files = null; }}>Close</button>
      </div>
      {#if filesLoading}
        <p class="muted">Searching…</p>
      {:else if files}
        <table>
          <thead><tr><th>File</th><th class="num">Last used</th><th class="num">Size</th></tr></thead>
          <tbody>
            {#each files as f}
              <tr><td><code class="path">{f.path}</code></td><td class="num">{ago(f.last_used)}</td><td class="num">{bytes(f.size)}</td></tr>
            {:else}
              <tr><td colspan="3" class="muted">No files match.</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
  {/if}
  </div>

  <div class="card">
    <div class="toolbar">
      <SearchBox bind:filter={entrySearch} placeholder="Filter this folder…" error={entryMatch.error} count={shownEntries.length} />
    </div>
    <table>
      <thead><tr><th>Name</th><th class="num">Size</th><th class="num">Files</th><th class="num">Share</th></tr></thead>
      <tbody>
        {#each shownEntries.slice(0, 200) as e}
          <tr>
            <td>
              {#if e.is_dir}
                <button class="link" onclick={() => open(e.path)} disabled={!!scanning}>📁 {e.name}</button>
              {:else}📄 {e.name}{/if}
            </td>
            <td class="num">{bytes(e.size)}</td>
            <td class="num">{e.files.toLocaleString()}</td>
            <td class="num">{pct(e.size, scan.total)}</td>
          </tr>
        {/each}
        {#if smaller > 0 && !entrySearch.pattern}
          <tr class="muted"><td>Smaller items (under 1 MB each)</td><td class="num">{bytes(smaller)}</td><td></td><td class="num">{pct(smaller, scan.total)}</td></tr>
        {/if}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .drives { grid-template-columns: repeat(auto-fill, minmax(420px, 1fr)); margin-bottom: 16px; }
  .card.current { border-color: var(--accent); }
  .usage { margin: 6px 0 0; }
  .usage strong { font-size: 18px; }
  .actions { display: flex; align-items: center; gap: 10px; margin-top: 12px; }
  .crumbs { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .grow { flex: 1; }
  h3 { margin: 0 0 12px; font-size: 14px; color: var(--muted); font-weight: 600; }
  .hint { border-left: 4px solid var(--warn); font-size: 13px; }
  .small { font-size: 12px; }
  .find { margin-bottom: 4px; }
</style>

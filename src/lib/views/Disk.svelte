<script lang="ts">
  import { api, type DriveInfo, type FileItem, type ScanResult } from "../api";
  import { ago, bytes, pct } from "../format";
  import Pie, { type Slice } from "../Pie.svelte";

  let drives = $state<DriveInfo[]>([]);
  let error = $state("");
  let scan = $state<ScanResult | null>(null);
  let scanning = $state("");
  // Scans are expensive; keep results so moving around the tree is instant.
  const cache = new Map<string, ScanResult>();

  let filesTitle = $state("");
  let files = $state<FileItem[] | null>(null);
  let filesLoading = $state(false);

  const ageColors = ["#34c38f", "#8bc34a", "#f7b924", "#ff9f43", "#e5484d"];

  $effect(() => {
    api.listDrives().then((d) => (drives = d)).catch((e) => (error = String(e)));
  });

  async function open(path: string, fresh = false) {
    files = null;
    filesTitle = "";
    error = "";
    const hit = !fresh && cache.get(path);
    if (hit) {
      scan = hit;
      return;
    }
    scanning = path;
    try {
      const res = await api.scanDir(path);
      cache.set(path, res);
      scan = res;
    } catch (e) {
      error = String(e);
    } finally {
      scanning = "";
    }
  }

  /** Cumulative path segments for the breadcrumb. */
  const crumbs = $derived.by(() => {
    if (!scan) return [];
    const p = scan.path;
    const sep = p.includes("\\") ? "\\" : "/";
    const parts = p.split(sep).filter(Boolean);
    const out: { label: string; path: string }[] = [];
    let acc = sep === "/" ? "/" : "";
    if (sep === "/") out.push({ label: "/", path: "/" });
    for (const part of parts) {
      acc = sep === "/" ? (acc.endsWith("/") ? acc + part : acc + "/" + part) : acc ? (acc.endsWith("\\") ? acc + part : acc + "\\" + part) : part + "\\";
      out.push({ label: part, path: acc });
    }
    return out;
  });

  async function showFiles(title: string, category: string | null, age: number | null) {
    if (!scan) return;
    filesTitle = title;
    filesLoading = true;
    files = null;
    try {
      files = await api.largestFiles(scan.path, category, age, 100);
    } catch (e) {
      error = String(e);
    } finally {
      filesLoading = false;
    }
  }

  const driveSlices = (d: DriveInfo): Slice[] => [
    { label: "Used", value: d.total - d.available, color: "#4f8cff" },
    { label: "Free", value: d.available, color: "#34c38f" },
  ];

  const folderSlices = $derived<Slice[]>(
    (scan?.entries ?? []).map((e) => ({
      label: e.name + (e.is_dir ? "/" : ""),
      value: e.size,
      key: e.path,
      clickable: e.is_dir,
    })),
  );
  const typeSlices = $derived<Slice[]>(
    (scan?.types ?? []).map((t) => ({ label: t.category, value: t.size, key: t.category })),
  );
  const ageSlices = $derived<Slice[]>(
    (scan?.ages ?? []).map((t, i) => ({ label: t.category, value: t.size, key: String(i), color: ageColors[i] })),
  );
  const oldShare = $derived(
    scan ? (scan.ages.slice(2).reduce((a, b) => a + b.size, 0) / (scan.total || 1)) * 100 : 0,
  );
</script>

<header>
  <h1>Disk usage</h1>
  <p class="muted">Pick a drive to see what is taking up space. Click any slice to dig deeper.</p>
</header>

{#if error}<p class="error">{error}</p>{/if}

{#if !scan}
  <div class="grid">
    {#each drives as d}
      <div class="card">
        <div class="row">
          <strong>{d.mount_point}</strong>
          <span class="muted">{d.name || d.kind} · {d.file_system}</span>
        </div>
        <Pie slices={driveSlices(d)} />
        <p class="muted">
          {bytes(d.total - d.available)} used of {bytes(d.total)} ({pct(d.total - d.available, d.total)})
        </p>
        <button class="primary" disabled={!!scanning} onclick={() => open(d.mount_point)}>
          {scanning === d.mount_point ? "Scanning…" : "Analyze"}
        </button>
      </div>
    {/each}
  </div>
  {#if scanning}
    <p class="muted">Scanning {scanning}. Large drives can take a minute or two.</p>
  {/if}
{:else}
  <div class="toolbar">
    <button onclick={() => (scan = null)} disabled={!!scanning}>Drives</button>
    <nav class="crumbs">
      {#each crumbs as c, i}
        {#if i > 0}<span class="muted">›</span>{/if}
        <button class="link" disabled={!!scanning} onclick={() => open(c.path)}>{c.label}</button>
      {/each}
    </nav>
    <span class="grow"></span>
    <span class="muted">{bytes(scan.total)} · {scan.files.toLocaleString()} files</span>
    {#if scan.skipped}<span class="muted" title="Folders we were not allowed to read">· {scan.skipped} unreadable</span>{/if}
    <button onclick={() => open(scan!.path, true)} disabled={!!scanning}>Rescan</button>
  </div>
  {#if scanning}<p class="muted">Scanning {scanning}…</p>{/if}

  <div class="grid three-pies">
    <div class="card">
      <Pie title="By folder" slices={folderSlices} max={12} onselect={(s) => s.key && open(s.key)} />
    </div>
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

  {#if filesTitle}
    <div class="card">
      <div class="row">
        <strong>{filesTitle}</strong>
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
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  {/if}

  <div class="card">
    <table>
      <thead><tr><th>Name</th><th class="num">Size</th><th class="num">Files</th><th class="num">Share</th></tr></thead>
      <tbody>
        {#each scan.entries.slice(0, 200) as e}
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
      </tbody>
    </table>
  </div>
{/if}

<style>
  .crumbs { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .grow { flex: 1; }
  .three-pies { grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); }
</style>

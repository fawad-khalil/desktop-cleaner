<script lang="ts">
  import { api, type DriveInfo, type ScanResult } from "../api";
  import { bytes, pct } from "../format";
  import Pie, { type Slice } from "../Pie.svelte";

  let drives = $state<DriveInfo[]>([]);
  let error = $state("");
  let scan = $state<ScanResult | null>(null);
  let scanning = $state("");
  let history = $state<string[]>([]);

  $effect(() => {
    api.listDrives().then((d) => (drives = d)).catch((e) => (error = String(e)));
  });

  async function open(path: string, push = true) {
    scanning = path;
    error = "";
    try {
      const res = await api.scanDir(path);
      if (push && scan) history = [...history, scan.path];
      scan = res;
    } catch (e) {
      error = String(e);
    } finally {
      scanning = "";
    }
  }

  function back() {
    const prev = history[history.length - 1];
    history = history.slice(0, -1);
    if (prev) open(prev, false);
    else scan = null;
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
    (scan?.types ?? []).map((t) => ({ label: t.category, value: t.size, clickable: false })),
  );
</script>

<header>
  <h1>Disk usage</h1>
  <p class="muted">Pick a drive to see what is taking up space. Click a folder slice to drill down.</p>
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
    <button onclick={back} disabled={!!scanning}>← Back</button>
    <code>{scan.path}</code>
    <span class="muted">{bytes(scan.total)} · {scan.files.toLocaleString()} files</span>
    {#if scan.skipped}<span class="muted" title="Folders we were not allowed to read">· {scan.skipped} folders not readable</span>{/if}
    {#if scanning}<span class="muted">Scanning {scanning}…</span>{/if}
  </div>
  <div class="grid two">
    <div class="card">
      <Pie title="By folder" slices={folderSlices} max={12} onselect={(s) => s.key && open(s.key)} />
    </div>
    <div class="card">
      <Pie title="By file type" slices={typeSlices} max={12} />
    </div>
  </div>
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

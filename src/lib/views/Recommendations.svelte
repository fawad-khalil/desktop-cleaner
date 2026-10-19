<script lang="ts">
  import { downloadDir } from "@tauri-apps/api/path";
  import {
    api, leftoverApi,
    type AppInfo, type CacheInfo, type DriveInfo, type FileItem, type Leftover, type ProcSnapshot,
  } from "../api";
  import { bytes, daysSince } from "../format";
  import { nav, type TabId } from "../nav.svelte";

  interface Rec {
    level: "high" | "medium" | "info";
    title: string;
    detail: string;
    /** Space this would free, when known; used for ranking. */
    gain?: number;
    tab?: TabId;
    action?: string;
  }

  interface Data {
    drives: DriveInfo[];
    caches: CacheInfo[];
    apps: AppInfo[];
    procs: ProcSnapshot;
    leftovers: Leftover[];
    oldDownloads: FileItem[];
  }

  let recs = $state<Rec[] | null>(null);
  let error = $state("");
  let loading = $state(false);

  const GB = 1024 ** 3;
  const MB = 1024 ** 2;

  /** Pure rule set: data in, recommendations out. */
  function build(d: Data): Rec[] {
    const out: Rec[] = [];

    for (const dr of d.drives) {
      const free = dr.available / (dr.total || 1);
      if (free < 0.1)
        out.push({ level: "high", title: `${dr.mount_point} is almost full`, detail: `Only ${bytes(dr.available)} free (${(free * 100).toFixed(0)}%). Windows and apps slow down and updates can fail when a system drive is this full.`, tab: "disk", action: "Find what's using space" });
      else if (free < 0.2)
        out.push({ level: "medium", title: `${dr.mount_point} is getting full`, detail: `${bytes(dr.available)} free (${(free * 100).toFixed(0)}%).`, tab: "disk", action: "Analyze drive" });
    }

    const lo = d.leftovers.filter((l) => l.size > 100 * MB);
    for (const l of lo)
      out.push({ level: l.size > 5 * GB ? "high" : "medium", title: `${l.title}: ${bytes(l.size)}`, detail: l.description, gain: l.size, tab: "cleanup", action: "Review leftovers" });

    const cacheTotal = d.caches.reduce((a, c) => a + c.size, 0);
    if (cacheTotal > 500 * MB)
      out.push({ level: cacheTotal > 5 * GB ? "high" : "medium", title: `${bytes(cacheTotal)} of caches can be cleared`, detail: `Largest: ${d.caches.slice(0, 3).map((c) => `${c.name} (${bytes(c.size)})`).join(", ")}. Apps rebuild caches as needed, so clearing them is safe, but the first launch afterwards may be slower.`, gain: cacheTotal, tab: "cleanup", action: "Clean caches" });

    const oldDl = d.oldDownloads.reduce((a, f) => a + f.size, 0);
    if (oldDl > 200 * MB)
      out.push({ level: "medium", title: `${bytes(oldDl)} in Downloads untouched for over a year`, detail: `${d.oldDownloads.length} files, e.g. ${d.oldDownloads.slice(0, 3).map((f) => f.path.split(/[\\/]/).pop()).join(", ")}. Installers and archives here are usually no longer needed.`, gain: oldDl, tab: "cleanup", action: "Find old files" });

    const stale = d.apps.filter((a) => (daysSince(a.last_used) ?? 0) > 180);
    if (stale.length) {
      const size = stale.reduce((a, x) => a + (x.size ?? 0), 0);
      out.push({ level: "info", title: `${stale.length} apps not used in 6+ months${size ? ` (${bytes(size)})` : ""}`, detail: stale.slice(0, 6).map((a) => a.name).join(", ") + (stale.length > 6 ? "…" : "") + ". Uninstall the ones you don't need from your system's app settings.", gain: size, tab: "apps", action: "See apps" });
    }
    for (const a of d.apps.filter((a) => (a.size ?? 0) > 5 * GB))
      out.push({ level: "info", title: `${a.name} uses ${bytes(a.size)}`, detail: "One of your largest apps. Consider uninstalling it if you no longer need it.", gain: a.size ?? 0, tab: "apps", action: "See apps" });

    const mem = d.procs.used_memory / (d.procs.total_memory || 1);
    const byMem = [...d.procs.processes].sort((a, b) => b.memory - a.memory);
    if (mem > 0.85)
      out.push({ level: "high", title: `Memory is ${(mem * 100).toFixed(0)}% used`, detail: `Top consumers: ${byMem.slice(0, 3).map((p) => `${p.name} (${bytes(p.memory)})`).join(", ")}. Closing them will speed things up.`, tab: "processes", action: "See processes" });
    const hog = [...d.procs.processes].sort((a, b) => b.cpu - a.cpu)[0];
    if (hog && hog.cpu > 50)
      out.push({ level: "medium", title: `${hog.name} is using ${hog.cpu.toFixed(0)}% CPU`, detail: "If you're not using it right now, it may be stuck or doing background work.", tab: "processes", action: "See processes" });

    if (!out.length) out.push({ level: "info", title: "All looks good", detail: "No issues found right now." });
    const rank = { high: 0, medium: 1, info: 2 };
    return out.sort((a, b) => rank[a.level] - rank[b.level] || (b.gain ?? 0) - (a.gain ?? 0));
  }

  async function load() {
    loading = true;
    error = "";
    try {
      const dl = await downloadDir().catch(() => "");
      const [drives, caches, apps, procs, leftovers, old1, old2] = await Promise.all([
        api.listDrives(), api.listCaches(), api.listApps(), api.listProcesses(), leftoverApi.find(),
        dl ? api.largestFiles(dl, null, 3, 500) : Promise.resolve([]),
        dl ? api.largestFiles(dl, null, 4, 500) : Promise.resolve([]),
      ]);
      recs = build({ drives, caches, apps, procs, leftovers, oldDownloads: [...old1, ...old2] });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  const totalGain = $derived((recs ?? []).filter((r) => r.tab === "cleanup").reduce((a, r) => a + (r.gain ?? 0), 0));
</script>

<header>
  <h1>Recommendations</h1>
  <p class="muted">Suggestions based on your drives, caches, update leftovers, downloads, applications and running processes.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if recs}
  <div class="toolbar">
    {#if totalGain}<strong>Up to {bytes(totalGain)} can be freed from the Cleanup tab.</strong>{/if}
    <span class="grow"></span>
    <button onclick={load} disabled={loading}>{loading ? "Analyzing…" : "Re-analyze"}</button>
  </div>
  {#each recs as r}
    <div class="card rec {r.level}">
      <div class="row">
        <strong>{r.title}</strong>
        {#if r.tab && r.action}<button onclick={() => (nav.tab = r.tab!)}>{r.action} →</button>{/if}
      </div>
      <p class="muted">{r.detail}</p>
    </div>
  {/each}
{:else}
  <p class="muted">Analyzing your system…</p>
{/if}

<style>
  .grow { flex: 1; }
</style>

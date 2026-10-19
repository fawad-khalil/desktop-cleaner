<script lang="ts">
  import { api, type AppInfo, type CacheInfo, type DriveInfo, type ProcSnapshot } from "../api";
  import { bytes, daysSince } from "../format";

  interface Rec {
    level: "high" | "medium" | "info";
    title: string;
    detail: string;
  }

  let recs = $state<Rec[] | null>(null);
  let error = $state("");

  const GB = 1024 ** 3;

  function build(drives: DriveInfo[], caches: CacheInfo[], apps: AppInfo[], procs: ProcSnapshot): Rec[] {
    const out: Rec[] = [];
    for (const d of drives) {
      const free = d.available / (d.total || 1);
      if (free < 0.1)
        out.push({ level: "high", title: `${d.mount_point} is almost full`, detail: `Only ${bytes(d.available)} free (${(free * 100).toFixed(0)}%). Start with the Disk tab to find the largest folders.` });
      else if (free < 0.2)
        out.push({ level: "medium", title: `${d.mount_point} is getting full`, detail: `${bytes(d.available)} free (${(free * 100).toFixed(0)}%).` });
    }
    const cacheTotal = caches.reduce((a, c) => a + c.size, 0);
    if (cacheTotal > GB)
      out.push({ level: "medium", title: `${bytes(cacheTotal)} of caches`, detail: `Largest: ${caches.slice(0, 3).map((c) => `${c.name} (${bytes(c.size)})`).join(", ")}. These are usually safe to clear.` });
    for (const c of caches.filter((c) => c.size > 2 * GB))
      out.push({ level: "info", title: `${c.name} is ${bytes(c.size)}`, detail: c.path });

    const stale = apps.filter((a) => (daysSince(a.last_used) ?? 0) > 180);
    if (stale.length)
      out.push({ level: "info", title: `${stale.length} apps not used in 6+ months`, detail: stale.slice(0, 6).map((a) => a.name).join(", ") + (stale.length > 6 ? "…" : "") });
    const bigApps = apps.filter((a) => (a.size ?? 0) > 5 * GB);
    for (const a of bigApps)
      out.push({ level: "info", title: `${a.name} uses ${bytes(a.size)}`, detail: "Consider uninstalling it if you no longer need it." });

    const mem = procs.used_memory / (procs.total_memory || 1);
    if (mem > 0.85) {
      const top = [...procs.processes].sort((a, b) => b.memory - a.memory).slice(0, 3);
      out.push({ level: "high", title: `Memory is ${(mem * 100).toFixed(0)}% used`, detail: `Top consumers: ${top.map((p) => `${p.name} (${bytes(p.memory)})`).join(", ")}.` });
    }
    if (!out.length) out.push({ level: "info", title: "All looks good", detail: "No issues found right now." });
    const rank = { high: 0, medium: 1, info: 2 };
    return out.sort((a, b) => rank[a.level] - rank[b.level]);
  }

  $effect(() => {
    Promise.all([api.listDrives(), api.listCaches(), api.listApps(), api.listProcesses()])
      .then(([d, c, a, p]) => (recs = build(d, c, a, p)))
      .catch((e) => (error = String(e)));
  });
</script>

<header>
  <h1>Recommendations</h1>
  <p class="muted">Suggestions based on your drives, caches, applications and memory use.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if recs}
  {#each recs as r}
    <div class="card rec {r.level}">
      <strong>{r.title}</strong>
      <p class="muted">{r.detail}</p>
    </div>
  {/each}
{:else}
  <p class="muted">Analyzing your system…</p>
{/if}

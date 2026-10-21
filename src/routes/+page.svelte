<script lang="ts">
  import Disk from "$lib/views/Disk.svelte";
  import Cleanup from "$lib/views/Cleanup.svelte";
  import Processes from "$lib/views/Processes.svelte";
  import Caches from "$lib/views/Caches.svelte";
  import Apps from "$lib/views/Apps.svelte";
  import Recommendations from "$lib/views/Recommendations.svelte";
  import Drives from "$lib/views/Drives.svelte";
  import Health from "$lib/views/Health.svelte";
  import { nav, type TabId } from "$lib/nav.svelte";
  import { cleaning, refreshCleaningStats } from "$lib/stats.svelte";
  import { bytes } from "$lib/format";
  import "../app.css";

  const tabs = [
    { id: "disk", label: "Disk", icon: "◔", view: Disk },
    { id: "cleanup", label: "Cleanup", icon: "✦", view: Cleanup },
    { id: "processes", label: "Processes", icon: "▤", view: Processes },
    { id: "caches", label: "Caches", icon: "◫", view: Caches },
    { id: "apps", label: "Apps", icon: "▦", view: Apps },
    { id: "recommendations", label: "Recommendations", icon: "★", view: Recommendations },
    { id: "health", label: "Health", icon: "♥", view: Health },
    { id: "drives", label: "Drives", icon: "⛁", view: Drives },
  ] as const satisfies readonly { id: TabId; label: string; icon: string; view: unknown }[];

  const active = $derived(nav.tab);
  // Views stay mounted after first visit so scans aren't lost when switching
  // tabs. Processes and Health are the exception: they poll, so they only
  // live while shown.
  let visited = $state<Set<TabId>>(new Set(["disk"]));

  $effect(() => {
    refreshCleaningStats();
  });

  function go(id: TabId) {
    nav.tab = id;
  }

  $effect(() => {
    if (!visited.has(nav.tab)) visited = new Set([...visited, nav.tab]);
  });
</script>

<div class="app">
  <nav>
    <div class="brand">DesktopCleaner</div>
    {#each tabs as t}
      <button class:active={active === t.id} onclick={() => go(t.id)}>
        <span class="icon">{t.icon}</span>{t.label}
      </button>
    {/each}
    {#if cleaning.stats}
      <div class="freed" title="Space freed by moving items to the Trash">
        <span>Freed this session</span><strong>{bytes(cleaning.stats.session_bytes)}</strong>
        <span>All time</span><strong>{bytes(cleaning.stats.total_bytes)}</strong>
      </div>
    {/if}
  </nav>
  <main>
    {#each tabs as t}
      {#if active === t.id || (visited.has(t.id) && t.id !== "processes" && t.id !== "health")}
        <section hidden={active !== t.id}>
          <t.view />
        </section>
      {/if}
    {/each}
  </main>
</div>

<style>
  .freed {
    margin-top: auto; padding: 12px 10px 4px; border-top: 1px solid #ffffff1a;
    display: grid; grid-template-columns: 1fr auto; gap: 4px 8px; font-size: 12px; color: var(--nav-text);
  }
  .freed strong { color: #fff; text-align: right; font-variant-numeric: tabular-nums; }
</style>

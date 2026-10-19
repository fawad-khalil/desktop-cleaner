<script lang="ts">
  import Disk from "$lib/views/Disk.svelte";
  import Cleanup from "$lib/views/Cleanup.svelte";
  import Processes from "$lib/views/Processes.svelte";
  import Caches from "$lib/views/Caches.svelte";
  import Apps from "$lib/views/Apps.svelte";
  import Recommendations from "$lib/views/Recommendations.svelte";
  import Drives from "$lib/views/Drives.svelte";
  import { nav, type TabId } from "$lib/nav.svelte";
  import "../app.css";

  const tabs = [
    { id: "disk", label: "Disk", icon: "◔", view: Disk },
    { id: "cleanup", label: "Cleanup", icon: "✦", view: Cleanup },
    { id: "processes", label: "Processes", icon: "▤", view: Processes },
    { id: "caches", label: "Caches", icon: "◫", view: Caches },
    { id: "apps", label: "Apps", icon: "▦", view: Apps },
    { id: "recommendations", label: "Recommendations", icon: "★", view: Recommendations },
    { id: "drives", label: "Drives", icon: "⛁", view: Drives },
  ] as const satisfies readonly { id: TabId; label: string; icon: string; view: unknown }[];

  const active = $derived(nav.tab);
  // Views stay mounted after first visit so scans aren't lost when switching
  // tabs. Processes is the exception: it polls, so it only lives while shown.
  let visited = $state<Set<TabId>>(new Set(["disk"]));

  function go(id: TabId) {
    nav.tab = id;
  }

  $effect(() => {
    if (!visited.has(nav.tab)) visited = new Set([...visited, nav.tab]);
  });
</script>

<div class="app">
  <nav>
    <div class="brand">StorageCleaner</div>
    {#each tabs as t}
      <button class:active={active === t.id} onclick={() => go(t.id)}>
        <span class="icon">{t.icon}</span>{t.label}
      </button>
    {/each}
  </nav>
  <main>
    {#each tabs as t}
      {#if active === t.id || (visited.has(t.id) && t.id !== "processes")}
        <section hidden={active !== t.id}>
          <t.view />
        </section>
      {/if}
    {/each}
  </main>
</div>

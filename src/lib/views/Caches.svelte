<script lang="ts">
  import { api, type CacheInfo } from "../api";
  import { bytes } from "../format";
  import Pie, { type Slice } from "../Pie.svelte";

  let caches = $state<CacheInfo[] | null>(null);
  let error = $state("");

  function load() {
    caches = null;
    api.listCaches().then((c) => (caches = c)).catch((e) => (error = String(e)));
  }
  $effect(load);

  const total = $derived((caches ?? []).reduce((a, c) => a + c.size, 0));
  const byGroup = $derived.by<Slice[]>(() => {
    const m = new Map<string, number>();
    for (const c of caches ?? []) m.set(c.group, (m.get(c.group) ?? 0) + c.size);
    return [...m].map(([label, value]) => ({ label, value }));
  });
  const byItem = $derived<Slice[]>((caches ?? []).map((c) => ({ label: c.name, value: c.size })));
</script>

<header>
  <h1>Caches</h1>
  <p class="muted">Known cache and temp locations for browsers, developer tools, apps and the OS.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if caches}
  <div class="toolbar">
    <strong>{bytes(total)}</strong><span class="muted">in {caches.length} cache locations</span>
    <button onclick={load}>Rescan</button>
  </div>
  <div class="grid two">
    <div class="card"><Pie title="By group" slices={byGroup} /></div>
    <div class="card"><Pie title="By location" slices={byItem} /></div>
  </div>
  <div class="card">
    <table>
      <thead><tr><th>Cache</th><th>Group</th><th>Path</th><th class="num">Files</th><th class="num">Size</th></tr></thead>
      <tbody>
        {#each caches as c}
          <tr>
            <td>{c.name}</td>
            <td>{c.group}</td>
            <td><code class="path">{c.path}</code></td>
            <td class="num">{c.files.toLocaleString()}</td>
            <td class="num">{bytes(c.size)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="muted">Measuring cache folders…</p>
{/if}

<script lang="ts">
  import { api, type CacheInfo } from "../api";
  import { bytes } from "../format";
  import Pie, { type Slice } from "../Pie.svelte";

  let caches = $state<CacheInfo[] | null>(null);
  let error = $state("");

  $effect(() => {
    api.listCaches().then((c) => (caches = c)).catch((e) => (error = String(e)));
  });

  const candidates = $derived((caches ?? []).filter((c) => c.size > 0));
  const total = $derived(candidates.reduce((a, c) => a + c.size, 0));
  const slices = $derived<Slice[]>(candidates.map((c) => ({ label: c.name, value: c.size })));
</script>

<header>
  <h1>Cleanup</h1>
  <p class="muted">Space that could be reclaimed from caches and temporary files.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if caches}
  <div class="grid two">
    <div class="card stat">
      <span class="muted">Potentially reclaimable</span>
      <strong>{bytes(total)}</strong>
      <span class="muted">across {candidates.length} locations</span>
      <p class="note">
        Nothing is deleted from this screen yet. Selective cleaning with a dry-run preview and
        Recycle Bin / Trash deletion arrives with the safety core.
      </p>
    </div>
    <div class="card"><Pie title="Where it is" slices={slices} /></div>
  </div>
{:else}
  <p class="muted">Looking for reclaimable space…</p>
{/if}

<script lang="ts">
  import { bytes, since } from "./format";
  import { cleaning, refreshCleaningStats } from "./stats.svelte";

  let { history = false }: { history?: boolean } = $props();
  let showHistory = $state(false);

  $effect(() => {
    refreshCleaningStats();
  });

  const s = $derived(cleaning.stats);
  const date = (secs: number) => new Date(secs * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
</script>

{#if s}
  <div class="grid freed">
    <div class="card stat">
      <span class="muted">Freed this session</span>
      <strong>{bytes(s.session_bytes)}</strong>
      <span class="muted">{s.session_items.toLocaleString()} item{s.session_items === 1 ? "" : "s"} moved to the Trash</span>
    </div>
    <div class="card stat">
      <span class="muted">Freed all time</span>
      <strong>{bytes(s.total_bytes)}</strong>
      <span class="muted">{s.since ? `since ${date(s.since)}` : "nothing cleaned yet"} · {s.total_items.toLocaleString()} items</span>
    </div>
  </div>
  {#if history && s.history.length}
    <p class="hist-toggle">
      <button class="link" onclick={() => (showHistory = !showHistory)}>
        {showHistory ? "Hide" : "Show"} recent cleanups ({s.history.length})
      </button>
    </p>
    {#if showHistory}
      <div class="card">
        <table>
          <thead><tr><th>When</th><th>What</th><th class="num">Items</th><th class="num">Freed</th></tr></thead>
          <tbody>
            {#each s.history as e}
              <tr>
                <td title={new Date(e.at * 1000).toLocaleString()}>{since(e.at)}</td>
                <td>{e.kind === "app" ? "App removed" : "Cleanup"}</td>
                <td class="num">{e.items.toLocaleString()}</td>
                <td class="num">{bytes(e.bytes)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
{/if}

<style>
  .freed { grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); margin-bottom: 12px; }
  .stat strong { font-size: 22px; }
  .hist-toggle { margin: 0 0 12px; font-size: 13px; }
</style>

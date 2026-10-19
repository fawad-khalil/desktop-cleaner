<script lang="ts">
  import { api, type ProcSnapshot } from "../api";
  import { bytes, pct } from "../format";

  let snap = $state<ProcSnapshot | null>(null);
  let error = $state("");
  let sortKey = $state<"memory" | "cpu" | "name">("memory");
  let filter = $state("");

  $effect(() => {
    const tick = () =>
      api.listProcesses().then((s) => (snap = s)).catch((e) => (error = String(e)));
    tick();
    const id = setInterval(tick, 2000);
    return () => clearInterval(id);
  });

  const rows = $derived.by(() => {
    const list = (snap?.processes ?? []).filter(
      (p) => !filter || p.name.toLowerCase().includes(filter.toLowerCase()),
    );
    return [...list].sort((a, b) =>
      sortKey === "name" ? a.name.localeCompare(b.name) : b[sortKey] - a[sortKey],
    );
  });
</script>

<header>
  <h1>Processes</h1>
  <p class="muted">Live view, refreshed every 2 seconds.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if snap}
  <div class="grid three">
    <div class="card stat"><span class="muted">CPU</span><strong>{snap.cpu.toFixed(1)}%</strong><span class="muted">{snap.cpu_count} logical cores</span></div>
    <div class="card stat"><span class="muted">Memory</span><strong>{bytes(snap.used_memory)}</strong><span class="muted">of {bytes(snap.total_memory)} ({pct(snap.used_memory, snap.total_memory)})</span></div>
    <div class="card stat"><span class="muted">Processes</span><strong>{snap.processes.length}</strong><span class="muted">running</span></div>
  </div>

  <div class="toolbar">
    <input placeholder="Filter by name…" bind:value={filter} />
    <span class="muted">Sort:</span>
    <select bind:value={sortKey}>
      <option value="memory">Memory</option>
      <option value="cpu">CPU</option>
      <option value="name">Name</option>
    </select>
  </div>

  <div class="card">
    <table>
      <thead>
        <tr><th>Name</th><th class="num">PID</th><th class="num">Parent</th><th class="num">CPU</th><th class="num">Memory</th><th class="num">Disk R/W per tick</th></tr>
      </thead>
      <tbody>
        {#each rows.slice(0, 300) as p (p.pid)}
          <tr title={p.exe ?? ""}>
            <td>{p.name}</td>
            <td class="num">{p.pid}</td>
            <td class="num">{p.parent ?? "—"}</td>
            <td class="num">{p.cpu.toFixed(1)}%</td>
            <td class="num">{bytes(p.memory)}</td>
            <td class="num">{bytes(p.disk_read)} / {bytes(p.disk_written)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="muted">Loading…</p>
{/if}

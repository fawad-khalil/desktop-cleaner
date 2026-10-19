<script lang="ts">
  import { api, type AppInfo } from "../api";
  import { ago, bytes } from "../format";

  let apps = $state<AppInfo[] | null>(null);
  let error = $state("");
  let filter = $state("");
  let sortKey = $state<"name" | "size" | "last_used">("size");

  $effect(() => {
    api.listApps().then((a) => (apps = a)).catch((e) => (error = String(e)));
  });

  const rows = $derived.by(() => {
    const f = filter.toLowerCase();
    const list = (apps ?? []).filter(
      (a) => !f || a.name.toLowerCase().includes(f) || (a.publisher ?? "").toLowerCase().includes(f),
    );
    return [...list].sort((a, b) => {
      if (sortKey === "name") return a.name.localeCompare(b.name);
      if (sortKey === "size") return (b.size ?? 0) - (a.size ?? 0);
      // Oldest use first; unknown at the end.
      return (a.last_used ?? Infinity) - (b.last_used ?? Infinity);
    });
  });
  const total = $derived((apps ?? []).reduce((a, x) => a + (x.size ?? 0), 0));
</script>

<header>
  <h1>Applications</h1>
  <p class="muted">Installed applications, their size and when they were last used (best effort).</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if apps}
  <div class="toolbar">
    <strong>{apps.length}</strong><span class="muted">apps · {bytes(total)} reported</span>
    <input placeholder="Filter…" bind:value={filter} />
    <span class="muted">Sort:</span>
    <select bind:value={sortKey}>
      <option value="size">Size</option>
      <option value="last_used">Least recently used</option>
      <option value="name">Name</option>
    </select>
  </div>
  <div class="card">
    <table>
      <thead><tr><th>Name</th><th>Publisher</th><th>Version</th><th class="num">Size</th><th class="num">Last used</th></tr></thead>
      <tbody>
        {#each rows as a}
          <tr title={a.location ?? ""}>
            <td>{a.name}</td>
            <td>{a.publisher ?? "—"}</td>
            <td>{a.version ?? "—"}</td>
            <td class="num">{bytes(a.size)}</td>
            <td class="num">{ago(a.last_used)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="muted">Loading installed apps…</p>
{/if}

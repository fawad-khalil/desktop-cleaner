<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type AppInfo, type AppLeftover, type RemoveOutcome } from "../api";
  import { ago, bytes } from "../format";

  type Col = "name" | "publisher" | "version" | "size" | "last_used";
  interface SortKey {
    col: Col;
    dir: 1 | -1;
  }

  const isMac = navigator.userAgent.includes("Mac");
  const isWin = navigator.userAgent.includes("Windows");

  let apps = $state<AppInfo[] | null>(null);
  let error = $state("");
  let filter = $state("");
  /** Sort keys in priority order; shift-click a header to add one. */
  let sort = $state<SortKey[]>([{ col: "size", dir: -1 }]);

  /** The app being removed and the leftovers offered for it. */
  let removing = $state<AppInfo | null>(null);
  let leftovers = $state<AppLeftover[] | null>(null);
  let pickedLeftovers = $state<Set<string>>(new Set());
  let busy = $state(false);
  let outcome = $state<{ app: string; res: RemoveOutcome } | null>(null);

  const load = () => api.listApps().then((a) => (apps = a)).catch((e) => (error = String(e)));
  $effect(() => {
    load();
  });

  const cols: { col: Col; label: string; num?: boolean; first: 1 | -1 }[] = [
    { col: "name", label: "Name", first: 1 },
    { col: "publisher", label: "Publisher", first: 1 },
    { col: "version", label: "Version", first: 1 },
    { col: "size", label: "Size", num: true, first: -1 },
    // Ascending last-used = least recently used first.
    { col: "last_used", label: "Last used", num: true, first: 1 },
  ];

  function clickHeader(col: Col, e: MouseEvent) {
    const first = cols.find((c) => c.col === col)!.first;
    const i = sort.findIndex((k) => k.col === col);
    if (e.shiftKey) {
      // Add as the next key, flip it, or drop it on the third click.
      if (i < 0) sort = [...sort, { col, dir: first }];
      else if (sort[i].dir === first) sort = sort.map((k, j) => (j === i ? { col, dir: (-first) as 1 | -1 } : k));
      else if (sort.length > 1) sort = sort.filter((_, j) => j !== i);
    } else {
      sort = [{ col, dir: i === 0 && sort.length === 1 ? ((-sort[0].dir) as 1 | -1) : first }];
    }
  }

  function cmp(a: AppInfo, b: AppInfo, col: Col): number {
    const va = a[col];
    const vb = b[col];
    // Unknown values always go last, whatever the direction.
    if (va == null && vb == null) return 0;
    if (va == null) return Infinity;
    if (vb == null) return -Infinity;
    if (typeof va === "number" && typeof vb === "number") return va - vb;
    return String(va).localeCompare(String(vb), undefined, { numeric: true, sensitivity: "base" });
  }

  const rows = $derived.by(() => {
    const f = filter.toLowerCase();
    const list = (apps ?? []).filter(
      (a) => !f || a.name.toLowerCase().includes(f) || (a.publisher ?? "").toLowerCase().includes(f),
    );
    return [...list].sort((a, b) => {
      for (const k of sort) {
        const c = cmp(a, b, k.col);
        if (c === Infinity || c === -Infinity) return c > 0 ? 1 : -1;
        if (c) return c * k.dir;
      }
      return 0;
    });
  });
  const total = $derived((apps ?? []).reduce((a, x) => a + (x.size ?? 0), 0));

  async function startRemove(a: AppInfo) {
    outcome = null;
    error = "";
    if (isMac) {
      removing = a;
      leftovers = null;
      pickedLeftovers = new Set();
      leftovers = await api.appLeftovers(a.location!).catch(() => []);
      pickedLeftovers = new Set(leftovers.map((l) => l.path));
      return;
    }
    const ok = await ask(
      isWin
        ? `Uninstall ${a.name}?\n\nIts own uninstaller will open; follow its steps to finish.`
        : `Find the command that uninstalls ${a.name}?`,
      { title: "Remove app", kind: "warning", okLabel: isWin ? "Open uninstaller" : "Continue", cancelLabel: "Cancel" },
    );
    if (ok) await doRemove(a, []);
  }

  async function confirmMacRemove() {
    if (!removing) return;
    const a = removing;
    const extra = [...pickedLeftovers];
    const ok = await ask(
      `Move ${a.name}${extra.length ? ` and ${extra.length} leftover item(s)` : ""} to the Trash?\n\nQuit the app first. You can put it back from the Trash until you empty it.`,
      { title: "Remove app", kind: "warning", okLabel: "Move to Trash", cancelLabel: "Cancel" },
    );
    if (ok) await doRemove(a, extra);
  }

  async function doRemove(a: AppInfo, extra: string[]) {
    busy = true;
    try {
      const res = await api.removeApp(isWin ? a.name : a.location!, extra);
      outcome = { app: a.name, res };
      removing = null;
      if (res.moved.length) load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const leftoverSize = $derived((leftovers ?? []).filter((l) => pickedLeftovers.has(l.path)).reduce((a, l) => a + l.size, 0));
</script>

<header>
  <h1>Applications</h1>
  <p class="muted">Installed applications, their size and when they were last used (best effort). Click a column to sort; Shift-click to sort by more columns.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if outcome}
  {@const r = outcome.res}
  <div class="card rec">
    {#if r.moved.length}
      <strong>Moved {outcome.app} to the Trash{r.moved.length > 1 ? ` with ${r.moved.length - 1} leftover item(s)` : ""}, freeing {bytes(r.freed)}.</strong>
    {:else if r.launched}
      <strong>{outcome.app}'s uninstaller has opened. Follow its steps to finish.</strong>
    {:else if r.command}
      <strong>Run this in a terminal to uninstall {outcome.app}:</strong>
      <p><code>{r.command}</code> <button class="link" onclick={() => navigator.clipboard.writeText(r.command!)}>Copy</button></p>
    {/if}
    {#if r.failed.length}
      <p class="muted">Couldn't move:</p>
      <ul>{#each r.failed as [p, why]}<li><code class="path">{p}</code>: {why}</li>{/each}</ul>
    {/if}
    <button class="link" onclick={() => (outcome = null)}>Dismiss</button>
  </div>
{/if}

{#if removing}
  <div class="card remove">
    <div class="row">
      <strong>Remove {removing.name}</strong>
      <button class="link" onclick={() => (removing = null)}>Cancel</button>
    </div>
    <p class="muted">{removing.location} · {bytes(removing.size)}</p>
    {#if leftovers === null}
      <p class="muted">Looking for files it left in your Library…</p>
    {:else if leftovers.length}
      <p>Also move these support files to the Trash ({bytes(leftoverSize)} selected):</p>
      {#each leftovers as l}
        <label class="check">
          <input type="checkbox" checked={pickedLeftovers.has(l.path)}
            onchange={() => { const s = new Set(pickedLeftovers); if (s.has(l.path)) s.delete(l.path); else s.add(l.path); pickedLeftovers = s; }} />
          <code class="path grow">{l.path}</code>
          <span class="muted">{bytes(l.size)}</span>
        </label>
      {/each}
    {:else}
      <p class="muted">No leftover support files found.</p>
    {/if}
    <div class="toolbar">
      <button class="danger" disabled={busy || leftovers === null} onclick={confirmMacRemove}>Move to Trash</button>
    </div>
  </div>
{/if}

{#if apps}
  <div class="toolbar">
    <strong>{apps.length}</strong><span class="muted">apps · {bytes(total)} reported</span>
    <input placeholder="Filter…" bind:value={filter} />
    {#if sort.length > 1}
      <button class="link" onclick={() => (sort = [sort[0]])}>Clear extra sorting</button>
    {/if}
  </div>
  <div class="card">
    <table>
      <thead>
        <tr>
          {#each cols as c}
            {@const i = sort.findIndex((k) => k.col === c.col)}
            <th class:num={c.num} aria-sort={i < 0 ? "none" : sort[i].dir > 0 ? "ascending" : "descending"}>
              <button class="sort" class:on={i >= 0} onclick={(e) => clickHeader(c.col, e)} title="Click to sort; Shift-click to add as another sort column">
                {c.label}
                {#if i >= 0}<span class="arrow">{sort[i].dir > 0 ? "▲" : "▼"}{#if sort.length > 1}<sup>{i + 1}</sup>{/if}</span>{/if}
              </button>
            </th>
          {/each}
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as a (a.location ?? a.name)}
          <tr title={a.location ?? ""}>
            <td>{a.name}</td>
            <td>{a.publisher ?? "—"}</td>
            <td>{a.version ?? "—"}</td>
            <td class="num">{bytes(a.size)}</td>
            <td class="num">{ago(a.last_used)}</td>
            <td class="num">
              {#if a.blocked}
                <span class="muted small" title={a.blocked}>{a.blocked === "built into macOS" ? "Built in" : "—"}</span>
              {:else}
                <button onclick={() => startRemove(a)} disabled={busy}>Remove</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="muted">Loading installed apps…</p>
{/if}

<style>
  .sort { all: unset; cursor: pointer; user-select: none; white-space: nowrap; }
  .sort:hover, .sort.on { color: var(--text); }
  .arrow { margin-left: 4px; font-size: 10px; color: var(--accent); }
  .remove { border-left: 4px solid var(--danger); }
  .check { display: flex; gap: 8px; align-items: center; padding: 3px 0; }
  .grow { flex: 1; }
  .small { font-size: 12px; }
</style>

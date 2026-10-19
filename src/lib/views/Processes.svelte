<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type ProcInfo, type ProcSnapshot } from "../api";
  import { bytes, mem, palette, pct } from "../format";
  import ProcessTree from "../ProcessTree.svelte";

  type Kind = ProcInfo["kind"];
  type Level = "High" | "Medium" | "Low";

  interface AppGroup {
    key: string;
    name: string;
    kind: Kind;
    procs: ProcInfo[];
    memory: number;
    /** Smoothed over recent refreshes so the numbers don't jump around. */
    cpu: number;
    connections: number;
    canQuit: boolean;
    impact: Level;
  }

  let snap = $state<ProcSnapshot | null>(null);
  let error = $state("");
  let notice = $state("");
  let view = $state<"simple" | "advanced">("simple");
  let sortKey = $state<"impact" | "memory" | "cpu" | "name">("impact");
  let filter = $state("");
  let showSystem = $state(false);
  let expanded = $state<Set<string>>(new Set());
  let paused = $state(false);
  /** Sections showing every row rather than the first few. */
  let showAll = $state<Set<Kind>>(new Set());
  const LIMIT = 12;
  const cpuAvg = new Map<string, number>();

  $effect(() => {
    const tick = () => {
      if (paused) return;
      api.listProcesses().then((s) => {
        // Smooth CPU per app across refreshes so the numbers don't jump around.
        const sums = new Map<string, number>();
        for (const p of s.processes) sums.set(`${p.kind}:${p.app}`, (sums.get(`${p.kind}:${p.app}`) ?? 0) + p.cpu);
        for (const [k, v] of sums) {
          const prev = cpuAvg.get(k);
          cpuAvg.set(k, prev == null ? v : prev * 0.5 + v * 0.5);
        }
        snap = s;
      }).catch((e) => (error = String(e)));
    };
    tick();
    const id = setInterval(tick, 2000);
    return () => clearInterval(id);
  });

  const GB = 1024 ** 3;

  function impactOf(memory: number, cpu: number, total: number): Level {
    const memShare = (memory / (total || 1)) * 100;
    if (cpu >= 30 || memShare >= 15 || memory >= 3 * GB) return "High";
    if (cpu >= 8 || memShare >= 5 || memory >= GB) return "Medium";
    return "Low";
  }

  const cpuLevel = (cpu: number): Level => (cpu >= 25 ? "High" : cpu >= 5 ? "Medium" : "Low");
  const rank: Record<Level, number> = { High: 3, Medium: 2, Low: 1 };

  const groups = $derived.by(() => {
    if (!snap) return [] as AppGroup[];
    const m = new Map<string, AppGroup>();
    for (const p of snap.processes) {
      const key = `${p.kind}:${p.app}`;
      let g = m.get(key);
      if (!g) {
        g = { key, name: p.app, kind: p.kind, procs: [], memory: 0, cpu: 0, connections: 0, canQuit: true, impact: "Low" };
        m.set(key, g);
      }
      g.procs.push(p);
      g.memory += p.memory;
      g.cpu += p.cpu;
      g.connections += p.connections;
      g.canQuit &&= p.mine && p.kind !== "system";
    }
    for (const g of m.values()) {
      g.cpu = cpuAvg.get(g.key) ?? g.cpu;
      g.impact = impactOf(g.memory, g.cpu, snap.total_memory);
      g.procs.sort((a, b) => b.memory - a.memory);
    }
    return [...m.values()];
  });

  function sorted(list: AppGroup[]) {
    const f = filter.toLowerCase();
    const out = f ? list.filter((g) => g.name.toLowerCase().includes(f) || g.procs.some((p) => p.name.toLowerCase().includes(f))) : [...list];
    const by = {
      impact: (a: AppGroup, b: AppGroup) => rank[b.impact] - rank[a.impact] || b.memory - a.memory,
      memory: (a: AppGroup, b: AppGroup) => b.memory - a.memory,
      cpu: (a: AppGroup, b: AppGroup) => b.cpu - a.cpu,
      name: (a: AppGroup, b: AppGroup) => a.name.localeCompare(b.name),
    }[sortKey];
    return out.sort(by);
  }

  const sections = $derived([
    { kind: "app" as Kind, title: "Apps you opened", blurb: "Programs you started. Quitting one you're not using frees its memory right away.", items: sorted(groups.filter((g) => g.kind === "app")) },
    { kind: "background" as Kind, title: "Running in the background", blurb: "Helpers, sync tools and agents that start on their own. Usually fine to leave running.", items: sorted(groups.filter((g) => g.kind === "background")) },
    ...(showSystem ? [{ kind: "system" as Kind, title: "System", blurb: "Part of your operating system. These can't be quit from here.", items: sorted(groups.filter((g) => g.kind === "system")) }] : []),
  ]);

  const systemCount = $derived(groups.filter((g) => g.kind === "system").length);

  /** Plain-language tips about the heaviest apps. */
  const tips = $derived.by(() => {
    if (!snap) return [];
    const quittable = groups.filter((g) => g.canQuit);
    const out: { text: string; group?: AppGroup }[] = [];
    const memUse = snap.used_memory / (snap.total_memory || 1);
    const byMem = [...quittable].sort((a, b) => b.memory - a.memory);
    for (const g of byMem.slice(0, 2)) {
      const share = g.memory / (snap.total_memory || 1);
      if (share >= 0.12 || (memUse > 0.8 && share >= 0.05))
        out.push({ text: `${g.name} is using ${mem(g.memory)} of memory (${pct(g.memory, snap.total_memory)} of your total). If you're not using it, quitting it will free that up.`, group: g });
    }
    const hog = [...quittable].sort((a, b) => b.cpu - a.cpu)[0];
    if (hog && hog.cpu >= 25 && !out.some((t) => t.group === hog))
      out.push({ text: `${hog.name} is keeping your processor busy (${hog.cpu.toFixed(0)}%). That drains battery and can make other apps feel slow.`, group: hog });
    if (memUse > 0.9 && !out.length)
      out.push({ text: "Memory is almost full. Closing a few apps or browser tabs will make your computer more responsive." });
    return out;
  });

  const memStatus = $derived.by(() => {
    if (!snap) return "";
    const u = snap.used_memory / (snap.total_memory || 1);
    return u < 0.7 ? "Plenty of memory free" : u < 0.85 ? "Memory is fairly busy" : "Memory is almost full";
  });
  const cpuStatus = $derived(!snap ? "" : snap.cpu < 25 ? "Mostly idle" : snap.cpu < 70 ? "Busy" : "Working hard");

  async function quit(g: AppGroup) {
    // Ask the app's top-level process(es) to quit; their helpers follow.
    const pids = new Set(g.procs.map((p) => p.pid));
    const roots = g.procs.filter((p) => p.parent == null || !pids.has(p.parent)).map((p) => p.pid);
    const ok = await ask(
      `Quit ${g.name}?\n\nIt will be asked to close normally. Unsaved work in it may be lost.`,
      { title: "Quit app", kind: "warning", okLabel: "Quit", cancelLabel: "Cancel" },
    );
    if (!ok) return;
    try {
      await api.quitProcesses(roots);
      notice = `Asked ${g.name} to quit.`;
      setTimeout(() => (notice = ""), 5000);
    } catch (e) {
      error = String(e);
    }
  }

  function toggle(key: string) {
    const s = new Set(expanded);
    if (s.has(key)) s.delete(key);
    else s.add(key);
    expanded = s;
  }

  function colorFor(name: string) {
    let h = 0;
    for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
    return palette[h % palette.length];
  }

  const rate = (b: number) => `${bytes(b)}/s`;
</script>

<header>
  <h1>What's running</h1>
  <p class="muted">Apps and background tasks using your computer right now, with the heaviest first. Updates every 2 seconds.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}
{#if notice}<p class="ok">{notice}</p>{/if}

{#if snap}
  <div class="grid three-up">
    <div class="card stat">
      <span class="muted">Memory</span>
      <strong>{mem(snap.used_memory)} <span class="of">of {mem(snap.total_memory)}</span></strong>
      <div class="bar"><div style:width={pct(snap.used_memory, snap.total_memory)} class:hot={snap.used_memory / snap.total_memory > 0.85}></div></div>
      <span class="muted">{memStatus}</span>
    </div>
    <div class="card stat">
      <span class="muted">Processor</span>
      <strong>{snap.cpu.toFixed(0)}% <span class="of">busy</span></strong>
      <div class="bar"><div style:width="{snap.cpu}%" class:hot={snap.cpu > 70}></div></div>
      <span class="muted">{cpuStatus}</span>
    </div>
    <div class="card stat">
      <span class="muted">Internet</span>
      <strong>↓ {rate(snap.net_rx_rate)}</strong>
      <span class="muted">↑ {rate(snap.net_tx_rate)} upload</span>
    </div>
  </div>

  {#if view === "simple" && tips.length}
    <div class="card tips">
      <strong>Suggestions</strong>
      {#each tips as t}
        <div class="tip">
          <span>{t.text}</span>
          {#if t.group}<button onclick={() => quit(t.group!)}>Quit {t.group.name}</button>{/if}
        </div>
      {/each}
    </div>
  {/if}

  <div class="toolbar">
    <div class="seg">
      <button class:on={view === "simple"} onclick={() => (view = "simple")}>Simple</button>
      <button class:on={view === "advanced"} onclick={() => (view = "advanced")}>Advanced</button>
    </div>
    {#if view === "simple"}
      <input placeholder="Search apps…" bind:value={filter} />
      <span class="muted">Sort by</span>
      <select bind:value={sortKey}>
        <option value="impact">Impact</option>
        <option value="memory">Memory</option>
        <option value="cpu">Processor</option>
        <option value="name">Name</option>
      </select>
      <label><input type="checkbox" bind:checked={showSystem} /> Show system processes ({systemCount})</label>
    {/if}
    <span class="grow"></span>
    <label><input type="checkbox" bind:checked={paused} /> Pause</label>
  </div>

  {#if view === "advanced"}
    <ProcessTree {snap} />
  {:else}
    {#each sections as sec}
      <div class="card">
        <div class="row">
          <h3>{sec.title} <span class="muted">· {sec.items.length}</span></h3>
          <span class="muted small">{sec.blurb}</span>
        </div>
        {#if !sec.items.length}
          <p class="muted">Nothing here{filter ? " matches your search" : ""}.</p>
        {:else}
          <table class="apps">
            <thead>
              <tr><th>Name</th><th>Impact</th><th>Memory</th><th>Processor</th><th>Internet</th><th></th></tr>
            </thead>
            <tbody>
              {#each showAll.has(sec.kind) || filter ? sec.items : sec.items.slice(0, LIMIT) as g (g.key)}
                <tr>
                  <td>
                    <button class="name" onclick={() => toggle(g.key)} title="Show the {g.procs.length} process(es) behind it">
                      <span class="avatar" style:background={colorFor(g.name)}>{g.name.charAt(0).toUpperCase()}</span>
                      <span>
                        <span class="title">{g.name}</span>
                        <span class="muted small">{g.procs.length === 1 ? "1 process" : `${g.procs.length} processes`} {expanded.has(g.key) ? "▾" : "▸"}</span>
                      </span>
                    </button>
                  </td>
                  <td><span class="badge {g.impact.toLowerCase()}">{g.impact}</span></td>
                  <td class="mem">
                    <span>{mem(g.memory)}</span>
                    <div class="bar thin"><div style:width={pct(g.memory, snap.total_memory)}></div></div>
                  </td>
                  <td><span class="lvl {cpuLevel(g.cpu).toLowerCase()}">{cpuLevel(g.cpu)}</span> <span class="muted small">{g.cpu.toFixed(1)}%</span></td>
                  <td>
                    {#if g.connections}
                      <span title="{g.connections} open connection(s)">Online</span>
                    {:else}<span class="muted">—</span>{/if}
                  </td>
                  <td class="num">
                    {#if g.canQuit}<button onclick={() => quit(g)}>Quit</button>{/if}
                  </td>
                </tr>
                {#if expanded.has(g.key)}
                  {#each g.procs as p (p.pid)}
                    <tr class="sub" title={p.exe ?? ""}>
                      <td>{p.name} <span class="muted small">#{p.pid}</span></td>
                      <td></td>
                      <td>{mem(p.memory)}</td>
                      <td class="muted">{p.cpu.toFixed(1)}%</td>
                      <td class="muted">{p.connections || ""}</td>
                      <td></td>
                    </tr>
                  {/each}
                {/if}
              {/each}
            </tbody>
          </table>
          {#if sec.items.length > LIMIT && !filter}
            <button class="link more" onclick={() => { const s = new Set(showAll); if (s.has(sec.kind)) s.delete(sec.kind); else s.add(sec.kind); showAll = s; }}>
              {showAll.has(sec.kind) ? "Show fewer" : `Show all ${sec.items.length}`}
            </button>
          {/if}
        {/if}
      </div>
    {/each}
    <p class="note">
      Impact combines memory and processor use. Quit asks an app to close normally, like choosing Quit from its menu;
      system processes are never offered. Exact internet usage per app isn't available without admin rights, so apps
      with open connections show as Online; total speed is shown above.
    </p>
  {/if}
{:else}
  <p class="muted">Loading…</p>
{/if}

<style>
  .three-up { grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); margin-bottom: 16px; }
  .stat strong { font-size: 22px; }
  .of { font-size: 14px; font-weight: 400; color: var(--muted); }
  .bar .hot { background: var(--danger); }
  .bar.thin { height: 4px; min-width: 60px; margin-top: 4px; }
  .tips { border-left: 4px solid var(--warn); }
  .tip { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-top: 8px; }
  .grow { flex: 1; }
  h3 { margin: 0; font-size: 15px; }
  .small { font-size: 12px; }
  .apps td { vertical-align: middle; }
  .apps th:nth-child(2), .apps td:nth-child(2) { width: 90px; }
  .apps th:nth-child(3), .apps td:nth-child(3) { width: 140px; }
  .apps th:nth-child(4), .apps td:nth-child(4) { width: 140px; }
  .apps th:nth-child(5), .apps td:nth-child(5) { width: 90px; }
  .apps td:last-child { width: 80px; }
  .name { all: unset; cursor: pointer; display: flex; align-items: center; gap: 10px; }
  .name > span:last-child { display: flex; flex-direction: column; }
  .title { font-weight: 600; }
  .avatar {
    width: 30px; height: 30px; border-radius: 8px; color: #fff; font-weight: 700;
    display: grid; place-items: center; flex: none;
  }
  .badge { font-size: 12px; font-weight: 600; padding: 2px 8px; border-radius: 10px; }
  .badge.high { background: #e5484d22; color: var(--danger); }
  .badge.medium { background: #f5a52422; color: #b7791f; }
  .badge.low { background: #34c38f22; color: #1f8a5f; }
  .lvl.high { color: var(--danger); font-weight: 600; }
  .lvl.medium { color: #b7791f; }
  .sub td { font-size: 12px; padding-top: 4px; padding-bottom: 4px; }
  .sub td:first-child { padding-left: 48px; }
  .ok { color: #1f8a5f; }
  .more { display: inline-block; margin-top: 10px; }
</style>

<script lang="ts">
  // Detailed process table: parent/child tree, per-executable groups or a
  // flat list, with raw numbers.
  import type { ProcInfo, ProcSnapshot } from "./api";
  import { mem } from "./format";

  interface Node {
    key: string;
    label: string;
    pid: number | null;
    count: number;
    memory: number;
    cpu: number;
    io: number;
    connections: number;
    listening: number;
    exe: string | null;
    children: Node[];
  }

  let { snap }: { snap: ProcSnapshot } = $props();

  let mode = $state<"tree" | "name" | "flat">("tree");
  let sortKey = $state<"memory" | "cpu" | "connections" | "io" | "label">("memory");
  let filter = $state("");
  let expanded = $state<Set<string>>(new Set());

  function leaf(p: ProcInfo): Node {
    return {
      key: String(p.pid), label: p.name, pid: p.pid, count: 1, memory: p.memory, cpu: p.cpu,
      io: p.disk_read + p.disk_written, connections: p.connections, listening: p.listening,
      exe: p.exe, children: [],
    };
  }

  function sum(n: Node, c: Node) {
    n.count += c.count; n.memory += c.memory; n.cpu += c.cpu; n.io += c.io;
    n.connections += c.connections; n.listening += c.listening;
  }

  function sortNodes(nodes: Node[]): Node[] {
    nodes.sort((a, b) => (sortKey === "label" ? a.label.localeCompare(b.label) : b[sortKey] - a[sortKey]));
    for (const n of nodes) sortNodes(n.children);
    return nodes;
  }

  /** Parent/child tree; each node's numbers include its whole subtree. */
  function buildTree(procs: ProcInfo[]): Node[] {
    const byPid = new Map(procs.map((p) => [p.pid, p]));
    const kids = new Map<number, ProcInfo[]>();
    const roots: ProcInfo[] = [];
    for (const p of procs) {
      if (p.parent != null && p.parent !== p.pid && byPid.has(p.parent)) {
        if (!kids.has(p.parent)) kids.set(p.parent, []);
        kids.get(p.parent)!.push(p);
      } else roots.push(p);
    }
    const seen = new Set<number>();
    const make = (p: ProcInfo): Node => {
      seen.add(p.pid);
      const n = leaf(p);
      for (const c of kids.get(p.pid) ?? []) {
        if (seen.has(c.pid)) continue; // guard against pid-reuse cycles
        const cn = make(c);
        n.children.push(cn);
        sum(n, cn);
      }
      return n;
    };
    return roots.map(make);
  }

  /** One row per executable name (e.g. all chrome.exe together). */
  function buildByName(procs: ProcInfo[]): Node[] {
    const m = new Map<string, Node>();
    for (const p of procs) {
      let g = m.get(p.name);
      if (!g) {
        g = { ...leaf(p), key: "name:" + p.name, pid: null, count: 0, memory: 0, cpu: 0, io: 0, connections: 0, listening: 0, children: [] };
        m.set(p.name, g);
      }
      const l = leaf(p);
      g.children.push(l);
      sum(g, l);
    }
    return [...m.values()];
  }

  const nodes = $derived.by(() => {
    const procs = snap.processes;
    const f = filter.toLowerCase();
    if (f) return sortNodes(procs.filter((p) => p.name.toLowerCase().includes(f)).map(leaf));
    if (mode === "tree") return sortNodes(buildTree(procs));
    if (mode === "name") return sortNodes(buildByName(procs));
    return sortNodes(procs.map(leaf));
  });

  const rows = $derived.by(() => {
    const out: { n: Node; depth: number }[] = [];
    const walk = (list: Node[], depth: number) => {
      for (const n of list) {
        out.push({ n, depth });
        if (expanded.has(n.key)) walk(n.children, depth + 1);
      }
    };
    walk(nodes, 0);
    return out.slice(0, 600);
  });

  function toggle(key: string) {
    const s = new Set(expanded);
    if (s.has(key)) s.delete(key);
    else s.add(key);
    expanded = s;
  }
</script>

<div class="toolbar">
  <div class="seg">
    <button class:on={mode === "tree"} onclick={() => (mode = "tree")}>By parent</button>
    <button class:on={mode === "name"} onclick={() => (mode = "name")}>By app</button>
    <button class:on={mode === "flat"} onclick={() => (mode = "flat")}>Flat</button>
  </div>
  <input placeholder="Filter by name…" bind:value={filter} />
  <span class="muted">Sort:</span>
  <select bind:value={sortKey}>
    <option value="memory">Memory</option>
    <option value="cpu">CPU</option>
    <option value="connections">Network connections</option>
    <option value="io">Disk I/O</option>
    <option value="label">Name</option>
  </select>
</div>

<div class="card">
  <table>
    <thead>
      <tr>
        <th>Name</th><th class="num">PID</th><th class="num">Procs</th><th class="num">CPU</th><th class="num">Memory</th>
        <th class="num" title="Disk bytes read + written in the last 2 s">Disk I/O</th>
        <th class="num" title="Established TCP connections (listening/UDP sockets)">Net conns</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as { n, depth } (n.key)}
        <tr title={n.exe ?? ""}>
          <td style:padding-left="{8 + depth * 18}px">
            {#if n.children.length && !filter}
              <button class="link tw" onclick={() => toggle(n.key)}>{expanded.has(n.key) ? "▾" : "▸"}</button>
            {:else}<span class="tw"></span>{/if}
            {n.label}
          </td>
          <td class="num">{n.pid ?? "—"}</td>
          <td class="num">{n.count}</td>
          <td class="num">{n.cpu.toFixed(1)}%</td>
          <td class="num">{mem(n.memory)}</td>
          <td class="num">{mem(n.io)}</td>
          <td class="num">{n.connections}{n.listening ? ` (${n.listening})` : ""}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if !snap.net_available}
    <p class="note">Couldn't read network sockets on this system, so connection counts are unavailable.</p>
  {:else}
    <p class="note">
      Operating systems don't report network bytes per process without admin rights or packet capture, so each
      process shows its open connections. Total throughput is shown above.
    </p>
  {/if}
</div>

<style>
  .tw { display: inline-block; width: 16px; }
</style>

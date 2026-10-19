<script lang="ts">
  import type { Entry } from "./api";
  import { bytes, palette, pct } from "./format";

  interface Extra {
    label: string;
    value: number;
    color: string;
    hint?: string;
  }
  interface Arc {
    d: string;
    color: string;
    opacity: number;
    label: string;
    value: number;
    entry?: Entry;
    hint?: string;
  }

  let {
    entries,
    total,
    name,
    extra = null,
    canUp = false,
    onselect,
    onup,
  }: {
    /** Direct children of the folder, each with its own largest children. */
    entries: Entry[];
    /** Size of the folder itself. */
    total: number;
    name: string;
    /** Used space outside the scanned files (e.g. unreadable system data). */
    extra?: Extra | null;
    canUp?: boolean;
    onselect: (e: Entry) => void;
    onup?: () => void;
  } = $props();

  const MAX = 14;
  const MIN_SHARE = 0.004;
  const smallColor = "#9aa5b1";
  const C = 160;
  const [R0, R1, R2, R3] = [58, 62, 118, 156];

  let hover = $state<{ label: string; value: number } | null>(null);

  const whole = $derived(total + (extra?.value ?? 0));

  /** Inner ring: the largest children, tiny ones folded into one slice. */
  const ring = $derived.by(() => {
    const sorted = [...entries].sort((a, b) => b.size - a.size);
    const big = sorted.filter((e, i) => i < MAX && e.size / (whole || 1) >= MIN_SHARE);
    const listed = big.reduce((a, e) => a + e.size, 0);
    const items: { label: string; value: number; color: string; entry?: Entry; hint?: string }[] = big.map((e, i) => ({
      label: e.name,
      value: e.size,
      color: palette[i % palette.length],
      entry: e,
    }));
    const small = Math.max(0, total - listed);
    if (small > 0) items.push({ label: "Smaller items", value: small, color: smallColor, hint: "Everything too small to show separately" });
    if (extra && extra.value > 0) items.push({ ...extra });
    return items;
  });

  function arc(r1: number, r2: number, a0: number, a1: number): string {
    if (a1 - a0 >= Math.PI * 2 - 1e-6) a1 = a0 + Math.PI * 2 - 1e-4;
    const p = (r: number, a: number) => `${C + r * Math.sin(a)},${C - r * Math.cos(a)}`;
    const large = a1 - a0 > Math.PI ? 1 : 0;
    return `M${p(r2, a0)} A${r2},${r2} 0 ${large} 1 ${p(r2, a1)} L${p(r1, a1)} A${r1},${r1} 0 ${large} 0 ${p(r1, a0)} Z`;
  }

  const arcs = $derived.by(() => {
    const inner: Arc[] = [];
    const outer: Arc[] = [];
    const k = (Math.PI * 2) / (whole || 1);
    let a = 0;
    for (const it of ring) {
      const a0 = a;
      a += it.value * k;
      inner.push({ d: arc(R1, R2, a0, a), color: it.color, opacity: 1, label: it.label, value: it.value, entry: it.entry, hint: it.hint });
      // Outer ring: this child's own largest items inside its slice.
      let b = a0;
      it.entry?.children?.forEach((g, j) => {
        const b0 = b;
        b += g.size * k;
        if (b - b0 < 0.012) return;
        outer.push({ d: arc(R2 + 3, R3, b0, b), color: it.color, opacity: j % 2 ? 0.45 : 0.68, label: `${it.label} › ${g.name}`, value: g.size, entry: g });
      });
    }
    return [...inner, ...outer];
  });

  const center = $derived(hover ?? { label: name || "This folder", value: whole });
</script>

<div class="sun">
  <svg viewBox="0 0 320 320" role="img" aria-label="Used space in {name}">
    {#each arcs as s}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <path
        d={s.d}
        fill={s.color}
        fill-opacity={s.opacity}
        class:click={s.entry?.is_dir}
        class:dim={hover !== null && hover.label !== s.label}
        onmouseenter={() => (hover = { label: s.label, value: s.value })}
        onmouseleave={() => (hover = null)}
        onclick={() => s.entry?.is_dir && onselect(s.entry)}
      ><title>{s.label}: {bytes(s.value)}{s.hint ? ` — ${s.hint}` : s.entry?.is_dir ? " — click to open" : ""}</title></path>
    {/each}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <circle cx={C} cy={C} r={R0} class="hole" class:click={canUp} onclick={() => canUp && onup?.()}>
      {#if canUp}<title>Go up one folder</title>{/if}
    </circle>
    <text x={C} y={C - 6} text-anchor="middle" class="big">{bytes(center.value)}</text>
    <text x={C} y={C + 12} text-anchor="middle" class="small">
      {center.label.length > 24 ? center.label.slice(0, 23) + "…" : center.label}
    </text>
    {#if hover}
      <text x={C} y={C + 28} text-anchor="middle" class="small">{pct(hover.value, whole)}</text>
    {:else if canUp}
      <text x={C} y={C + 30} text-anchor="middle" class="small up">↑ up</text>
    {/if}
  </svg>
  <ul>
    {#each ring as it}
      <li class:dim={hover !== null && hover.label !== it.label}
        onmouseenter={() => (hover = { label: it.label, value: it.value })}
        onmouseleave={() => (hover = null)}>
        <span class="sw" style:background={it.color}></span>
        {#if it.entry?.is_dir}
          <button class="link" onclick={() => onselect(it.entry!)} title="Open {it.label}">{it.label}</button>
        {:else}
          <span class="lbl" title={it.hint ?? it.label}>{it.label}</span>
        {/if}
        <span class="val">{bytes(it.value)}</span>
        <span class="share">{pct(it.value, whole)}</span>
      </li>
    {/each}
  </ul>
</div>

<style>
  .sun { display: flex; gap: 20px; align-items: center; flex-wrap: wrap; }
  svg { width: 320px; height: 320px; flex: none; max-width: 100%; }
  path { stroke: var(--panel); stroke-width: 1; transition: opacity 0.15s; }
  .click { cursor: pointer; }
  .dim { opacity: 0.35; }
  .hole { fill: var(--panel); }
  .hole.click:hover { fill: var(--bg); }
  .big { font-size: 20px; font-weight: 700; fill: var(--text); pointer-events: none; }
  .small { font-size: 11px; fill: var(--muted); pointer-events: none; }
  .up { fill: var(--accent); }
  ul { list-style: none; margin: 0; padding: 0; flex: 1; min-width: 240px; }
  li { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; transition: opacity 0.15s; }
  .sw { width: 10px; height: 10px; border-radius: 3px; flex: none; }
  .lbl, .link { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .val { color: var(--muted); font-variant-numeric: tabular-nums; }
  .share { color: var(--muted); font-variant-numeric: tabular-nums; width: 48px; text-align: right; }
</style>

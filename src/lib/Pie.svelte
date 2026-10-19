<script module lang="ts">
  export interface Slice {
    label: string;
    value: number;
    color?: string;
    /** Slices that can't be drilled into are drawn but not clickable. */
    clickable?: boolean;
    /** Arbitrary payload for the caller (e.g. a path). */
    key?: string;
  }
</script>

<script lang="ts">
  import { bytes, palette, pct } from "./format";

  let {
    slices,
    title = "",
    max = 10,
    format = bytes,
    onselect,
  }: {
    slices: Slice[];
    title?: string;
    max?: number;
    format?: (n: number) => string;
    onselect?: (s: Slice, index: number) => void;
  } = $props();

  let hover = $state<number | null>(null);

  // Keep the chart readable: top `max` slices, the rest folded into "Other".
  const shown = $derived.by(() => {
    const sorted = slices.filter((s) => s.value > 0).sort((a, b) => b.value - a.value);
    const top = sorted.slice(0, max).map((s, i) => ({
      ...s,
      color: s.color ?? palette[i % palette.length],
    }));
    const rest = sorted.slice(max).reduce((a, s) => a + s.value, 0);
    if (rest > 0) top.push({ label: "Other", value: rest, color: "#9aa5b1", clickable: false });
    return top;
  });

  const total = $derived(shown.reduce((a, s) => a + s.value, 0));

  const R = 90;
  const r = 56;

  function arc(start: number, end: number): string {
    // A full circle can't be drawn as a single arc; split it.
    if (end - start >= Math.PI * 2 - 1e-6) end = start + Math.PI * 2 - 1e-4;
    const p = (rad: number, a: number) => [100 + rad * Math.sin(a), 100 - rad * Math.cos(a)];
    const [x1, y1] = p(R, start);
    const [x2, y2] = p(R, end);
    const [x3, y3] = p(r, end);
    const [x4, y4] = p(r, start);
    const large = end - start > Math.PI ? 1 : 0;
    return `M${x1},${y1} A${R},${R} 0 ${large} 1 ${x2},${y2} L${x3},${y3} A${r},${r} 0 ${large} 0 ${x4},${y4} Z`;
  }

  const paths = $derived.by(() => {
    let a = 0;
    return shown.map((s) => {
      const start = a;
      a += total ? (s.value / total) * Math.PI * 2 : 0;
      return arc(start, a);
    });
  });
</script>

<div class="pie">
  {#if title}<h3>{title}</h3>{/if}
  {#if total === 0}
    <p class="muted">Nothing to show.</p>
  {:else}
    <div class="body">
      <svg viewBox="0 0 200 200" role="img" aria-label={title}>
        {#each shown as s, i}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <path
            d={paths[i]}
            fill={s.color}
            class:dim={hover !== null && hover !== i}
            class:click={onselect && s.clickable !== false}
            onmouseenter={() => (hover = i)}
            onmouseleave={() => (hover = null)}
            onclick={() => s.clickable !== false && onselect?.(s, i)}
          />
        {/each}
        <text x="100" y="96" text-anchor="middle" class="big">
          {hover !== null ? pct(shown[hover].value, total) : format(total)}
        </text>
        <text x="100" y="114" text-anchor="middle" class="small">
          {hover !== null ? shown[hover].label.slice(0, 22) : "total"}
        </text>
      </svg>
      <ul>
        {#each shown as s, i}
          <li
            class:dim={hover !== null && hover !== i}
            onmouseenter={() => (hover = i)}
            onmouseleave={() => (hover = null)}
          >
            <span class="sw" style:background={s.color}></span>
            {#if onselect && s.clickable !== false}
              <button class="link" onclick={() => onselect?.(s, i)} title={s.label}>{s.label}</button>
            {:else}
              <span class="lbl" title={s.label}>{s.label}</span>
            {/if}
            <span class="val">{format(s.value)}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .pie h3 { margin: 0 0 8px; font-size: 14px; color: var(--muted); font-weight: 600; }
  .body { display: flex; gap: 16px; align-items: center; flex-wrap: wrap; }
  svg { width: 200px; height: 200px; flex: none; }
  path { transition: opacity 0.15s; stroke: var(--panel); stroke-width: 1; }
  path.click { cursor: pointer; }
  .dim { opacity: 0.35; }
  .big { font-size: 18px; font-weight: 700; fill: var(--text); }
  .small { font-size: 10px; fill: var(--muted); }
  ul { list-style: none; margin: 0; padding: 0; flex: 1; min-width: 180px; }
  li { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; transition: opacity 0.15s; }
  .sw { width: 10px; height: 10px; border-radius: 3px; flex: none; }
  .lbl, .link { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .val { color: var(--muted); font-variant-numeric: tabular-nums; }
</style>

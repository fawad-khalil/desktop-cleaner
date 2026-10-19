<script module lang="ts">
  export interface Segment {
    label: string;
    value: number;
    color: string;
    hint?: string;
  }
</script>

<script lang="ts">
  import { bytes } from "./format";

  let { segments, total }: { segments: Segment[]; total: number } = $props();

  let hover = $state<number | null>(null);
  const shown = $derived(segments.filter((s) => s.value > 0));
</script>

<!-- A single row like macOS Settings › Storage: used space by kind, then free. -->
<div class="bar" role="img" aria-label={shown.map((s) => `${s.label} ${bytes(s.value)}`).join(", ")}>
  {#each shown as s, i}
    <div
      class="seg"
      class:dim={hover !== null && hover !== i}
      style:width="{(s.value / (total || 1)) * 100}%"
      style:background={s.color}
      title="{s.label}: {bytes(s.value)}{s.hint ? ` — ${s.hint}` : ''}"
      onmouseenter={() => (hover = i)}
      onmouseleave={() => (hover = null)}
      role="presentation"
    ></div>
  {/each}
</div>
<ul class="legend">
  {#each shown as s, i}
    <li
      class:dim={hover !== null && hover !== i}
      onmouseenter={() => (hover = i)}
      onmouseleave={() => (hover = null)}
      title={s.hint ?? ""}
    >
      <span class="sw" style:background={s.color}></span>{s.label}
      <span class="muted">{bytes(s.value)}</span>
    </li>
  {/each}
</ul>

<style>
  .bar {
    display: flex; height: 22px; border-radius: 6px; overflow: hidden;
    background: var(--border); margin: 10px 0 8px;
  }
  .seg { height: 100%; min-width: 2px; transition: opacity 0.15s; }
  .seg + .seg { box-shadow: inset 1px 0 0 var(--panel); }
  .legend { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 12px; }
  .legend li { display: flex; align-items: center; gap: 6px; transition: opacity 0.15s; }
  .sw { width: 9px; height: 9px; border-radius: 50%; flex: none; }
  .sw[style*="--border"] { outline: 1px solid var(--muted); }
  .dim { opacity: 0.35; }
</style>

<script lang="ts">
  import type { SearchFilter } from "./search";

  let {
    filter = $bindable(),
    placeholder = "Search name or path…",
    error = "",
    count = null,
    onselect,
    onunselect,
  }: {
    filter: SearchFilter;
    placeholder?: string;
    /** Inline message for an invalid pattern. */
    error?: string;
    /** "N matching" shown next to the box, when given. */
    count?: number | null;
    /** Shows "Select matching" / "Unselect matching" when provided. */
    onselect?: () => void;
    onunselect?: () => void;
  } = $props();
</script>

<div class="search">
  <div class="box" class:bad={!!error}>
    <input bind:value={filter.pattern} {placeholder} spellcheck="false" aria-label="Search" aria-invalid={!!error} />
    <button class="opt" class:on={filter.regex} onclick={() => (filter.regex = !filter.regex)}
      title="Use a regular expression (e.g. \.log$ or ^/Users/.*/Downloads)" aria-pressed={filter.regex}>.*</button>
    <button class="opt" class:on={filter.case_sensitive} onclick={() => (filter.case_sensitive = !filter.case_sensitive)}
      title="Match upper/lower case exactly" aria-pressed={filter.case_sensitive}>Aa</button>
    {#if filter.pattern}
      <button class="opt" onclick={() => (filter.pattern = "")} title="Clear search" aria-label="Clear search">✕</button>
    {/if}
  </div>
  {#if filter.pattern && !error && count != null}
    <span class="muted small">{count.toLocaleString()} matching</span>
  {/if}
  {#if filter.pattern && !error && onselect}
    <button class="link small" onclick={onselect}>Select matching</button>
    {#if onunselect}<button class="link small" onclick={onunselect}>Unselect matching</button>{/if}
  {/if}
  {#if error}<span class="err small">Invalid pattern: {error}</span>{/if}
</div>

<style>
  .search { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .box { display: flex; align-items: center; border: 1px solid var(--border); border-radius: 8px; background: var(--panel); padding-right: 2px; }
  .box.bad { border-color: var(--danger); }
  .box input { border: none; background: transparent; min-width: 220px; outline: none; }
  .opt { border: none; background: transparent; padding: 3px 6px; font-size: 12px; font-family: ui-monospace, monospace; color: var(--muted); border-radius: 5px; }
  .opt.on { background: var(--accent); color: #fff; }
  .small { font-size: 12px; }
  .err { color: var(--danger); }
</style>

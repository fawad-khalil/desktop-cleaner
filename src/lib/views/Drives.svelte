<script lang="ts">
  import { formatApi, type Volume } from "../api";
  import { bytes } from "../format";

  let volumes = $state<Volume[]>([]);
  let error = $state("");
  let target = $state<Volume | null>(null);
  let fs = $state("");
  let label = $state("");
  let typed = $state("");
  let busy = $state(false);
  let done = $state("");

  function load() {
    formatApi.list().then((v) => (volumes = v)).catch((e) => (error = String(e)));
  }
  $effect(load);

  function start(v: Volume) {
    target = v;
    fs = v.file_systems[0] ?? "";
    label = "";
    typed = "";
    error = "";
    done = "";
  }

  async function run() {
    if (!target || typed !== target.confirm_text) return;
    busy = true;
    error = "";
    try {
      await formatApi.format(target.mount_point, fs, label, typed);
      done = `${target.mount_point} was formatted as ${fs}.`;
      target = null;
      load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<header>
  <h1>Drives</h1>
  <p class="muted">
    Mounted volumes. Non-system drives can be formatted on Windows and Linux. The system drive and
    anything the OS needs are always locked.
  </p>
</header>
{#if error && !target}<p class="error">{error}</p>{/if}
{#if done}<div class="card rec"><strong>{done}</strong></div>{/if}

<div class="card">
  <table>
    <thead>
      <tr><th>Drive</th><th>Device / label</th><th>File system</th><th>Removable</th><th class="num">Size</th><th></th></tr>
    </thead>
    <tbody>
      {#each volumes as v}
        <tr>
          <td><code>{v.mount_point}</code></td>
          <td>{v.device || "—"}</td>
          <td>{v.file_system}</td>
          <td>{v.removable ? "Yes" : "No"}</td>
          <td class="num">{bytes(v.total)}</td>
          <td class="num">
            {#if v.blocked}
              <span class="muted locked" title={v.blocked}>🔒 {v.blocked}</span>
            {:else}
              <button class="danger" onclick={() => start(v)}>Format…</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if target}
  <div class="backdrop" role="presentation" onclick={() => !busy && (target = null)}></div>
  <div class="modal card" role="dialog" aria-modal="true" aria-label="Format drive">
    <h2>Format {target.mount_point}</h2>
    <p class="warnbox">
      <strong>Everything on {target.mount_point}{target.device ? ` (${target.device})` : ""}, {bytes(target.total)}, will be permanently erased.</strong>
      Formatting does not use the Recycle Bin / Trash and cannot be undone. Back up anything you want to keep first.
    </p>
    <div class="form">
      <label>File system
        <select bind:value={fs}>
          {#each target.file_systems as f}<option value={f}>{f}</option>{/each}
        </select>
      </label>
      <label>New label
        <input bind:value={label} maxlength="32" placeholder="optional" />
      </label>
      <label><span>Type <code>{target.confirm_text}</code> to confirm</span>
        <input bind:value={typed} autocomplete="off" spellcheck="false" />
      </label>
    </div>
    <p class="note">Your system will ask for administrator permission. Nothing happens if you decline.</p>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="toolbar end">
      <button onclick={() => (target = null)} disabled={busy}>Cancel</button>
      <button class="danger" disabled={busy || typed !== target.confirm_text} onclick={run}>
        {busy ? "Formatting…" : "Erase and format"}
      </button>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: #0008; z-index: 10; }
  .modal { position: fixed; z-index: 11; top: 50%; left: 50%; transform: translate(-50%, -50%); width: min(520px, 92vw); }
  .modal h2 { margin: 0 0 12px; font-size: 18px; }
  .warnbox { border-left: 4px solid var(--danger); padding: 8px 12px; background: color-mix(in srgb, var(--danger) 10%, transparent); border-radius: 6px; }
  .form { display: grid; gap: 12px; margin: 14px 0; }
  .form label { display: grid; gap: 4px; font-size: 13px; color: var(--muted); }
  .locked { display: inline-block; max-width: 260px; white-space: normal; text-align: right; }
  .end { justify-content: flex-end; margin: 0; }
</style>

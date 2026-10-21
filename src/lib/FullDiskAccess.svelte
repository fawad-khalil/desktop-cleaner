<script lang="ts">
  import { api } from "./api";
  import { access, checkAccess, isMac, notNow } from "./privacy.svelte";

  let opened = $state(false);

  $effect(() => {
    checkAccess();
    // Coming back from System Settings: see whether it was granted.
    const onFocus = () => {
      if (access.granted === false) checkAccess(true);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function grant() {
    opened = true;
    await api.openFullDiskAccess().catch(() => {});
  }
</script>

{#if isMac && access.granted === false && !access.notNow}
  <div class="card fda">
    <strong>Grant Full Disk Access once</strong>
    <p>
      Without it, macOS asks again for each protected place DesktopCleaner looks at (other apps' data, Contacts,
      Photos, drives…), and forgets your answers when the app is updated. With it, there are no more prompts and
      scans see everything. Until then, protected folders are skipped and count as <em>System &amp; other</em>.
    </p>
    <ol>
      <li>Click <em>Open Full Disk Access settings</em>.</li>
      <li>Turn on <strong>DesktopCleaner</strong>. If it isn't listed, click <strong>+</strong> and pick it from Applications.</li>
      <li>When macOS offers <em>Quit &amp; Reopen</em>, accept it.</li>
    </ol>
    <div class="actions">
      <button class="primary" onclick={grant}>Open Full Disk Access settings</button>
      <button onclick={notNow}>Not now</button>
      {#if opened}<span class="muted">Waiting for access… this card goes away once it's on.</span>{/if}
    </div>
  </div>
{/if}

<style>
  .fda { border-left: 4px solid var(--accent); }
  p { margin: 6px 0; }
  ol { margin: 6px 0 12px; padding-left: 20px; }
  .actions { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
</style>

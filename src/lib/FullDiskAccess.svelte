<script lang="ts">
  import { api } from "./api";
  import { access, checkAccess, isMac, notNow } from "./privacy.svelte";

  let opened = $state(false);
  /** "Check again" found no access: explain why instead of waiting. */
  let stillMissing = $state(false);
  let checking = $state(false);

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

  async function checkAgain() {
    checking = true;
    await checkAccess(true);
    checking = false;
    stillMissing = access.granted === false;
  }

  /** Who the Settings switch has to be turned on for. */
  const target = $derived(access.host ?? "DesktopCleaner");
</script>

{#if isMac && access.granted === false && !access.notNow}
  <div class="card fda">
    <strong>Grant Full Disk Access once</strong>
    <p>
      Without it, macOS asks again for each protected place DesktopCleaner looks at (other apps' data, Contacts,
      Photos, drives…), and forgets your answers when the app is updated. With it, there are no more prompts and
      scans see everything. Until then, protected folders are skipped and count as <em>System &amp; other</em>.
    </p>
    {#if access.host}
      <p class="warn">
        DesktopCleaner was started from <strong>{access.host}</strong> (a development run), so macOS uses
        {access.host}'s permissions. Grant Full Disk Access to <strong>{access.host}</strong>, then restart it.
      </p>
    {/if}
    <ol>
      <li>Click <em>Open Full Disk Access settings</em>.</li>
      <li>Turn on <strong>{target}</strong>. If it isn't listed, click <strong>+</strong> and pick it from Applications.</li>
      <li>When macOS offers <em>Quit &amp; Reopen</em>, accept it.</li>
    </ol>
    {#if stillMissing}
      <p class="warn">
        {#if access.unsealed}
          This copy of DesktopCleaner isn't signed in a way macOS can match to the switch in System Settings, so
          turning it on has no effect. Install a newer build (or one you signed yourself, see the README), then in
          Full Disk Access remove the old DesktopCleaner entry with <strong>−</strong>, add the new one with
          <strong>+</strong>, and reopen the app.
        {:else if access.host}
          {access.host} doesn't have Full Disk Access yet. Turn it on for {access.host} and restart {access.host}.
        {:else}
          macOS still reports no access. It applies only after the app is reopened: quit DesktopCleaner (⌘Q) and open
          it again. If the switch is already on, turn it off and on again, or remove the entry with <strong>−</strong>
          and add the app again.
        {/if}
      </p>
    {/if}
    <div class="actions">
      <button class="primary" onclick={grant}>Open Full Disk Access settings</button>
      <button onclick={checkAgain} disabled={checking}>{checking ? "Checking…" : "I've granted it, check again"}</button>
      <button onclick={notNow}>Continue without</button>
      {#if opened && !stillMissing}<span class="muted">This card goes away once access is on.</span>{/if}
    </div>
  </div>
{/if}

<style>
  .fda { border-left: 4px solid var(--accent); }
  p { margin: 6px 0; }
  .warn { border-left: 3px solid var(--warn); padding-left: 8px; }
  ol { margin: 6px 0 12px; padding-left: 20px; }
  .actions { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
</style>

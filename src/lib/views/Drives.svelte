<script lang="ts">
  import { api, type DriveInfo } from "../api";
  import { bytes, pct } from "../format";

  let drives = $state<DriveInfo[]>([]);
  let error = $state("");

  $effect(() => {
    api.listDrives().then((d) => (drives = d)).catch((e) => (error = String(e)));
  });
</script>

<header>
  <h1>Drives</h1>
  <p class="muted">Mounted volumes. Formatting of non-system drives (Windows and Linux) comes in a later step.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

<div class="card">
  <table>
    <thead>
      <tr><th>Mount</th><th>Label</th><th>File system</th><th>Type</th><th>Removable</th><th class="num">Size</th><th class="num">Free</th><th>Usage</th></tr>
    </thead>
    <tbody>
      {#each drives as d}
        <tr>
          <td><code>{d.mount_point}</code></td>
          <td>{d.name || "—"}</td>
          <td>{d.file_system}</td>
          <td>{d.kind}</td>
          <td>{d.removable ? "Yes" : "No"}</td>
          <td class="num">{bytes(d.total)}</td>
          <td class="num">{bytes(d.available)}</td>
          <td>
            <div class="bar"><div style:width={pct(d.total - d.available, d.total)}></div></div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

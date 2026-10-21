<script lang="ts">
  import { api, healthApi, type Health, type ProcSnapshot, type Sensor } from "../api";
  import { bytes } from "../format";

  let h = $state<Health | null>(null);
  let procs = $state<ProcSnapshot | null>(null);
  let error = $state("");

  // Every 5 s while this screen is open (it unmounts when hidden), so the
  // readings cost nothing in the background.
  $effect(() => {
    const tick = () => {
      healthApi.get().then((x) => (h = x)).catch((e) => (error = String(e)));
      api.listProcesses().then((p) => (procs = p)).catch(() => {});
    };
    tick();
    const id = setInterval(tick, 5000);
    return () => clearInterval(id);
  });

  const avg = (s: Sensor[]) => (s.length ? s.reduce((a, x) => a + x.celsius, 0) / s.length : null);
  const max = (s: Sensor[]) => (s.length ? Math.max(...s.map((x) => x.celsius)) : null);
  const deg = (c: number | null | undefined) => (c == null ? "—" : `${c.toFixed(0)} °C`);
  const num = (v: number | null | undefined, unit: string, digits = 1) => (v == null ? "—" : `${v.toFixed(digits)} ${unit}`);
  const heat = (c: number) => (c >= 90 ? "hot" : c >= 75 ? "warm" : "ok");
  const mins = (m: number) => `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
  const date = (secs: number) => new Date(secs * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short" });

  /** Apps using the most power right now (macOS reports it per process). */
  const topPower = $derived.by(() => {
    if (!procs) return [];
    const m = new Map<string, number>();
    for (const p of procs.processes) if (p.power_w != null) m.set(p.app, (m.get(p.app) ?? 0) + p.power_w);
    return [...m].filter(([, w]) => w >= 0.05).sort((a, b) => b[1] - a[1]).slice(0, 8);
  });
  const hasPower = $derived(!!procs?.processes.some((p) => p.power_w != null));

  /** Time to empty at the current draw, when running on battery. */
  const drainEstimate = $derived.by(() => {
    const b = h?.battery;
    if (!b || b.plugged_in || b.power_w == null || b.power_w >= -0.1) return null;
    if (b.max_mah == null || b.percent == null || b.voltage_v == null) return null;
    const wh = (b.max_mah / 1000) * (b.percent / 100) * b.voltage_v;
    return Math.round((wh / -b.power_w) * 60);
  });

  // Small inline charts.
  const W = 320;
  const H = 90;
  function path(points: [number, number][], x0: number, x1: number, y0: number, y1: number) {
    if (points.length < 2) return "";
    const sx = (x: number) => ((x - x0) / (x1 - x0 || 1)) * W;
    const sy = (y: number) => H - ((y - y0) / (y1 - y0 || 1)) * H;
    return points.map(([x, y], i) => `${i ? "L" : "M"}${sx(x).toFixed(1)},${sy(y).toFixed(1)}`).join(" ");
  }
  const healthChart = $derived.by(() => {
    const hist = h?.history ?? [];
    if (hist.length < 2) return null;
    const lo = Math.min(75, ...hist.map((r) => r.health_pct));
    const x0 = hist[0].day, x1 = hist[hist.length - 1].day;
    return { d: path(hist.map((r) => [r.day, r.health_pct]), x0, x1, lo, 100), y80: H - ((80 - lo) / (100 - lo)) * H };
  });
  const drainChart = $derived.by(() => {
    const d = h?.drain ?? [];
    if (d.length < 2) return null;
    return { d: path(d.map((s) => [s.at, s.percent]), d[0].at, d[d.length - 1].at, 0, 100), minutes: Math.round((d[d.length - 1].at - d[0].at) / 60) };
  });
</script>

<header>
  <h1>Health</h1>
  <p class="muted">Temperatures, fans, battery and storage health, from what this computer reports without admin rights. Updates every 5 seconds.</p>
</header>
{#if error}<p class="error">{error}</p>{/if}

{#if !h}
  <p class="muted">Reading sensors…</p>
{:else}
  <div class="grid cards">
    <!-- CPU -->
    <div class="card">
      <h3>Processor temperature</h3>
      {#if h.temps.cpu.length}
        <div class="big">
          <span class={heat(max(h.temps.cpu)!)}>{deg(avg(h.temps.cpu))}</span>
          <span class="muted small">average · max {deg(max(h.temps.cpu))}</span>
        </div>
        <ul class="sensors">
          {#each h.temps.cpu as s}
            <li><span class="lbl" title={s.label}>{s.label}</span>
              <span class="bar"><span class={heat(s.celsius)} style:width="{Math.min(100, s.celsius)}%"></span></span>
              <span class="val">{deg(s.celsius)}</span></li>
          {/each}
        </ul>
        <p class="note">Sensor names are the hardware's own; on Apple Silicon each "tdie" sensor sits on the CPU die rather than one per core.</p>
      {:else}
        <p class="na">Not available: {h.temps.note ?? "no processor temperature sensor is exposed."}</p>
      {/if}
    </div>

    <!-- GPU -->
    <div class="card">
      <h3>Graphics {h.gpu.name ? `· ${h.gpu.name}` : ""}</h3>
      {#if h.gpu.utilization != null}
        <div class="big"><span>{h.gpu.utilization.toFixed(0)}%</span><span class="muted small">busy</span></div>
        <div class="meter"><div style:width="{h.gpu.utilization}%"></div></div>
      {:else}
        <p class="na">Utilization not available: {h.gpu.note}</p>
      {/if}
      {#if h.temps.gpu.length}
        <p>Temperature <strong class={heat(max(h.temps.gpu)!)}>{deg(avg(h.temps.gpu))}</strong> <span class="muted small">average · max {deg(max(h.temps.gpu))} ({h.temps.gpu.length} sensors)</span></p>
      {:else}
        <p class="na">Temperature not available on this computer.</p>
      {/if}
    </div>

    <!-- Fans -->
    <div class="card">
      <h3>Fans</h3>
      {#if h.fans.fans.length}
        {#each h.fans.fans as f}
          <div class="fan">
            <strong>Fan {f.index + 1}</strong>
            <span class="pill" class:on={f.rpm > 0}>{f.rpm > 0 ? "On" : "Off"}</span>
            <span class="grow">{Math.round(f.rpm).toLocaleString()} RPM</span>
            {#if f.max_rpm}<span class="muted small">{Math.round(f.min_rpm ?? 0).toLocaleString()}–{Math.round(f.max_rpm).toLocaleString()}</span>{/if}
          </div>
          {#if f.max_rpm}<div class="meter"><div style:width="{Math.min(100, (f.rpm / f.max_rpm) * 100)}%"></div></div>{/if}
        {/each}
      {:else}
        <p class="na">{h.fans.note}</p>
      {/if}
    </div>

    <!-- Battery -->
    <div class="card">
      <h3>Battery</h3>
      {#if h.battery}
        {@const b = h.battery}
        <div class="big">
          <span>{b.percent == null ? "—" : `${b.percent.toFixed(0)}%`}</span>
          <span class="muted small">
            {b.charging ? "Charging" : b.fully_charged ? "Charged" : b.plugged_in ? "Plugged in, not charging" : "On battery"}
            {#if b.minutes_to_empty}· {mins(b.minutes_to_empty)} left{/if}
            {#if b.minutes_to_full}· full in {mins(b.minutes_to_full)}{/if}
          </span>
        </div>
        <dl>
          <dt>Health</dt><dd class:warnc={b.health_pct != null && b.health_pct < 80}>{num(b.health_pct, "%")} <span class="muted small">({num(b.max_mah, "mAh", 0)} of {num(b.design_mah, "mAh", 0)} design)</span></dd>
          <dt>Condition</dt><dd>{b.condition ?? "—"}</dd>
          <dt>Cycles</dt><dd>{b.cycles ?? "—"}{b.design_cycles ? ` of ${b.design_cycles} rated` : ""}</dd>
          <dt>Temperature</dt><dd>{deg(b.temp_c)}</dd>
          <dt>Voltage · current</dt><dd>{num(b.voltage_v, "V", 2)} · {num(b.current_a, "A", 2)}</dd>
          <dt>Power</dt><dd>{b.power_w == null ? "—" : b.power_w > 0.05 ? `charging at ${b.power_w.toFixed(1)} W` : b.power_w < -0.05 ? `using ${(-b.power_w).toFixed(1)} W` : "idle (0 W)"}</dd>
        </dl>
      {:else}
        <p class="na">{h.battery_note}</p>
      {/if}
    </div>

    <!-- Power adapter -->
    <div class="card">
      <h3>Power</h3>
      <div class="big">
        <span>{num(h.power.system_power_w, "W")}</span><span class="muted small">this computer is using now</span>
      </div>
      {#if h.power.adapter_connected}
        <dl>
          <dt>Adapter</dt><dd>{h.power.adapter_name ?? "Connected"}</dd>
          {#if h.power.adapter_watts != null}<dt>Rated</dt><dd>{num(h.power.adapter_watts, "W", 0)}</dd>{/if}
          {#if h.power.adapter_voltage_v != null}<dt>Voltage · current</dt><dd>{num(h.power.adapter_voltage_v, "V")} · {num(h.power.adapter_current_a, "A", 2)}</dd>{/if}
        </dl>
      {:else}
        <p class="muted">No power adapter connected.</p>
      {/if}
      {#if h.power.note}<p class="note">{h.power.note}</p>{/if}
    </div>

    <!-- Storage -->
    <div class="card">
      <h3>Storage</h3>
      {#each h.storage as d}
        <p><strong>{d.name}</strong></p>
        <dl>
          <dt>SMART status</dt><dd class:warnc={!!d.smart_status && !/verified|passed|healthy|ok/i.test(d.smart_status)}>{d.smart_status ?? "Not available"}</dd>
          <dt>Temperature</dt><dd>{deg(d.temp_c)}</dd>
          {#if d.wear_pct != null}<dt>Wear</dt><dd>{d.wear_pct.toFixed(0)}% of rated life used</dd>{/if}
          {#if d.data_written_bytes != null}<dt>Data written</dt><dd>{bytes(d.data_written_bytes)}</dd>{/if}
          {#if d.power_on_hours != null}<dt>Powered on</dt><dd>{d.power_on_hours.toLocaleString()} h</dd>{/if}
        </dl>
        {#if d.note}<p class="note">{d.note}</p>{/if}
      {:else}
        <p class="na">{h.storage_note ?? "No disks found."}</p>
      {/each}
    </div>
  </div>

  {#if h.battery}
    <div class="grid two">
      <div class="card">
        <h3>Battery health over time</h3>
        {#if healthChart}
          <svg viewBox="0 0 {W} {H}" class="chart" role="img" aria-label="Battery health history">
            <line x1="0" x2={W} y1={healthChart.y80} y2={healthChart.y80} class="ref" />
            <path d={healthChart.d} class="line" />
          </svg>
          <p class="muted small">Dashed line: 80%, when batteries are usually considered worn.</p>
        {:else}
          <p class="muted">Recorded once a day; the chart appears after the second day.</p>
        {/if}
        {#if h.projection}
          <p>
            <strong>Estimate:</strong>
            {#if h.projection.date_80}health reaches 80% around <strong>{date(h.projection.date_80)}</strong>{/if}
            {#if h.projection.date_80 && h.projection.cycles_80} (about {h.projection.cycles_80} cycles){:else if h.projection.cycles_80}health reaches 80% at about <strong>{h.projection.cycles_80} cycles</strong>{h.battery.cycles ? ` (${Math.max(0, h.projection.cycles_80 - h.battery.cycles)} from now)` : ""}{/if}.
          </p>
          <p class="note">{h.projection.basis} Estimates assume your usage stays the same.</p>
        {/if}
      </div>

      <div class="card">
        <h3>Battery drain</h3>
        {#if drainEstimate != null}
          <p>At the current draw ({(-(h.battery.power_w ?? 0)).toFixed(1)} W), the battery lasts about <strong>{mins(drainEstimate)}</strong> <span class="muted small">(estimate)</span>.</p>
        {:else if h.battery.plugged_in}
          <p class="muted">Plugged in, so the battery isn't draining.</p>
        {:else}
          <p class="muted">Waiting for a power reading…</p>
        {/if}
        {#if drainChart}
          <svg viewBox="0 0 {W} {H}" class="chart" role="img" aria-label="Battery level this session">
            <path d={drainChart.d} class="line" />
          </svg>
          <p class="muted small">Battery level over the last {drainChart.minutes} min (sampled every 30 s while this app runs).</p>
        {:else}
          <p class="muted small">A chart of the battery level builds up while the app runs.</p>
        {/if}
      </div>
    </div>
  {/if}

  <div class="grid two">
    <div class="card">
      <h3>Using the most power</h3>
      {#if !hasPower}
        <p class="na">Not available: this system doesn't report energy use per app without admin rights.</p>
      {:else if !topPower.length}
        <p class="muted">Nothing is using noticeable power right now.</p>
      {:else}
        <ul class="sensors">
          {#each topPower as [app, w]}
            <li><span class="lbl">{app}</span>
              <span class="bar"><span class="ok" style:width="{Math.min(100, (w / topPower[0][1]) * 100)}%"></span></span>
              <span class="val">{w.toFixed(2)} W</span></li>
          {/each}
        </ul>
        <p class="note">Energy billed to each app's processes over the last few seconds (your own apps only; system processes aren't readable).</p>
      {/if}
    </div>
    <div class="card">
      <h3>Other sensors</h3>
      {#if h.temps.ram.length}
        <p>Memory {deg(avg(h.temps.ram))}</p>
      {:else}
        <p class="na">Memory temperature: {h.ram_temp_note}</p>
      {/if}
      {#if h.temps.other.length || h.temps.battery.length}
        <ul class="sensors">
          {#each [...h.temps.battery, ...h.temps.other] as s}
            <li><span class="lbl">{s.label}</span><span class="val">{deg(s.celsius)}</span></li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .cards { grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); margin-bottom: 16px; }
  h3 { margin: 0 0 10px; font-size: 14px; color: var(--muted); font-weight: 600; }
  .big { display: flex; align-items: baseline; gap: 8px; margin-bottom: 8px; }
  .big > span:first-child { font-size: 26px; font-weight: 700; }
  .small { font-size: 12px; }
  .na { color: var(--muted); font-size: 13px; font-style: italic; }
  .sensors { list-style: none; margin: 0; padding: 0; font-size: 12px; }
  .sensors li { display: flex; align-items: center; gap: 8px; padding: 2px 0; }
  .lbl { width: 120px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val { width: 64px; text-align: right; font-variant-numeric: tabular-nums; }
  .sensors .bar { flex: 1; height: 6px; background: var(--border); border-radius: 3px; overflow: hidden; min-width: 40px; }
  .sensors .bar > span { display: block; height: 100%; }
  .ok { background: #34c38f; color: inherit; }
  .warm { background: #f5a524; }
  .hot { background: var(--danger); }
  .big .ok, .big .warm, .big .hot, strong.ok, strong.warm, strong.hot { background: none; }
  .big .warm, strong.warm { color: #b7791f; }
  .big .hot, strong.hot { color: var(--danger); }
  .meter { height: 8px; background: var(--border); border-radius: 4px; overflow: hidden; margin: 4px 0 10px; }
  .meter > div { height: 100%; background: var(--accent); }
  .fan { display: flex; align-items: center; gap: 8px; }
  .grow { flex: 1; }
  .pill { font-size: 11px; padding: 1px 7px; border-radius: 9px; background: var(--border); color: var(--muted); }
  .pill.on { background: #34c38f22; color: #1f8a5f; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 6px 0; font-size: 13px; }
  dt { color: var(--muted); }
  dd { margin: 0; }
  .warnc { color: var(--danger); font-weight: 600; }
  .chart { width: 100%; height: 90px; display: block; margin: 6px 0; }
  .line { fill: none; stroke: var(--accent); stroke-width: 2; }
  .ref { stroke: var(--danger); stroke-dasharray: 4 4; stroke-width: 1; }
</style>

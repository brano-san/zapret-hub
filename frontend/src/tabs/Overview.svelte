<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type ProxyStatus, type RunSummary } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshOverview } from "$lib/app.svelte";
  import { latestPerSuite, ms, runDate } from "$lib/format";
  import { LINKS, S } from "$lib/strings";

  let proxy = $state<ProxyStatus | null>(null);
  let proxyBusy = $state(false);
  let upToDate = $state(false);
  let runs = $state<RunSummary[]>([]);

  // The newest run of each kind: a DPI run does not hide the last standard one.
  const latest = $derived(latestPerSuite(runs));

  $effect(() => {
    void app.overview?.last_run?.id;
    void call(S.failed.runs, commands.listRuns()).then((list) => (runs = list ?? []));
  });

  async function checkProxy(): Promise<void> {
    const addr = app.settings?.socks5;
    if (!addr) return;
    proxyBusy = true;
    proxy = (await call(S.failed.proxy, commands.checkProxy(addr))) ?? null;
    proxyBusy = false;
  }

  async function checkUpdate(): Promise<void> {
    upToDate = (await change(S.failed.version, () => commands.checkFlowsealUpdate())) === null;
  }

  onMount(() => {
    void refreshOverview();
    void checkProxy();
  });
</script>

{#if app.overview}
  {@const o = app.overview}
  {@const locked = app.busy || o.testing}
  {#if o.folder_error && (o.folder || !o.zapret2.dir)}
    <section class="card">
      <h2 class="bad">{S.overview.folderProblem}</h2>
      <pre>{o.folder_error}</pre>
      <div class="buttons"><button onclick={() => (app.tab = "settings")}>{S.overview.chooseFolder}</button></div>
    </section>
  {/if}
  <div class="cards">
    <section class="card">
      <h2>{S.overview.service(o.service_name)}</h2>
      <p class={o.service === "Running" ? "ok" : "bad"}>{S.svc[o.service]}</p>
      <p>{S.overview.strategy}: <strong>{o.strategy ?? S.overview.noStrategy}</strong></p>
      {#if o.service === "Running" && !o.winws_running}<p class="warn">{S.overview.winwsMissing}</p>{/if}
      {#if o.service === "StopPending" || o.windivert === "StopPending"}<p class="warn">{S.overview.stopPending}</p>{/if}
      <p>
        {S.overview.quick}:
        {#if o.quick}
          <span class={o.quick.ok ? "ok" : "bad"}>{o.quick.ok ? S.overview.quickOk : S.overview.quickFailed}</span>
        {:else}
          <span class="muted">{S.overview.quickNever}</span>
        {/if}
      </p>
      {#if o.quick}
        {#each o.quick.checks as item (item.name)}
          <p class="muted">{item.name} — {item.status === "Ok" ? ms(item.time_ms) : S.status[item.status]}</p>
        {/each}
      {/if}
      <div class="buttons">
        <button disabled={locked} onclick={() => change(S.failed.restart, () => commands.restartZapret())}>
          {S.overview.restart}
        </button>
        <button disabled={locked} onclick={() => change(S.failed.quick, () => commands.quickCheck())}>{S.overview.quick}</button>
      </div>
    </section>

    <!-- zapret2 alone: no Flowseal folder, nothing of it to show. -->
    {#if o.folder}
      <section class="card">
        <h2>{S.overview.flowseal}</h2>
        <p>{S.overview.version(o.version)}</p>
        {#if o.update}
          {@const version = o.update}
          <p class="warn">{S.overview.updateAvailable(version)}</p>
          <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.release(version)))}>
            {S.overview.release}
          </button>
        {:else if upToDate}
          <p class="ok">{S.overview.upToDate}</p>
        {/if}
        <div class="buttons"><button disabled={app.busy} onclick={checkUpdate}>{S.overview.checkUpdate}</button></div>
      </section>
    {/if}

    <section class="card">
      <h2>{o.zapret2.dir ? S.overview.zapret2 : S.overview.zapret2Off}</h2>
      {#if !o.zapret2.dir}
        <p class="muted">{S.overview.zapret2OffHint}</p>
        <div class="buttons"><button onclick={() => (app.tab = "settings")}>{S.overview.zapret2Connect}</button></div>
      {:else if o.zapret2.error}
        <pre class="bad">{o.zapret2.error}</pre>
      {:else}
        <p>{S.overview.version(o.zapret2.version)}</p>
      {/if}
    </section>

    <section class="card">
      <h2>{S.overview.proxy}</h2>
      {#if !app.settings?.socks5}
        <p class="muted">{S.overview.proxyNotSet}</p>
      {:else}
        <p>{app.settings.socks5}</p>
        {#if proxyBusy}
          <p class="muted">{S.common.checking}</p>
        {:else if proxy}
          <p class={proxy.ok ? "ok" : "bad"}>{proxy.ok ? S.overview.proxyOk(ms(proxy.latency_ms)) : S.overview.proxyFailed}</p>
          {#if proxy.error}<pre>{proxy.error}</pre>{/if}
        {/if}
        <div class="buttons"><button disabled={proxyBusy} onclick={checkProxy}>{S.common.check}</button></div>
      {/if}
    </section>

    <section class="card">
      <h2>{S.overview.lastTest}</h2>
      {#each latest as run (run.id)}
        <h3>{run.suite === "Standard" ? S.tests.standard : S.tests.dpi}</h3>
        {#if run.best && run.best_score}
          {@const score = run.best_score}
          <p>{S.overview.best}: <strong>{run.best}</strong></p>
          <p>
            {run.suite === "Standard"
              ? S.overview.errors(score.total - score.ok, score.total, ms(score.median_ms))
              : S.strategies.dpiScore(score.likely_blocked, score.fail)}
          </p>
        {/if}
        <p class="muted">{runDate(run.id)}</p>
      {:else}
        <p class="muted">{S.overview.noTests}</p>
      {/each}
      <div class="buttons">
        <button
          class="primary"
          disabled={locked || !o.last_run?.best}
          onclick={() => change(S.failed.apply, () => commands.applyBest())}
        >
          {S.overview.applyBest}
        </button>
        <button disabled={o.testing} onclick={() => (app.tab = "tests")}>{S.overview.runTests}</button>
      </div>
    </section>
  </div>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

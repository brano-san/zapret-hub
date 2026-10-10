<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type ProxyStatus } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshOverview } from "$lib/app.svelte";
  import { ms, runDate } from "$lib/format";
  import { LINKS, S } from "$lib/strings";

  let proxy = $state<ProxyStatus | null>(null);
  let proxyBusy = $state(false);
  let upToDate = $state(false);

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
  {#if o.folder_error}
    <section class="card">
      <h2 class="bad">{S.overview.folderProblem}</h2>
      <pre>{o.folder_error}</pre>
      <div class="buttons"><button onclick={() => (app.tab = "settings")}>{S.overview.chooseFolder}</button></div>
    </section>
  {/if}
  <div class="cards">
    <section class="card">
      <h2>{S.overview.service}</h2>
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
      {#if o.last_run?.best && o.last_run.best_score}
        {@const score = o.last_run.best_score}
        <p>{S.overview.best}: <strong>{o.last_run.best}</strong></p>
        <p>{S.overview.errors(score.total - score.ok, score.total, ms(score.median_ms))}</p>
        <p class="muted">{runDate(o.last_run.id)}</p>
      {:else}
        <p class="muted">{S.overview.noTests}</p>
      {/if}
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

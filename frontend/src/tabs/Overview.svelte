<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type ProfileScore, type ProxyStatus, type RunSummary } from "$lib/ipc/bindings";
  import TgWsCard from "../components/TgWsCard.svelte";
  import { call } from "$lib/api";
  import { app, change, refreshOverview } from "$lib/app.svelte";
  import { latestPerSuite, ms, runDate, ZAPRET2 } from "$lib/format";
  import { S } from "$lib/strings";

  let proxy = $state<ProxyStatus | null>(null);
  let proxyBusy = $state(false);
  let runs = $state<RunSummary[]>([]);

  // The newest run of each kind: a DPI run does not hide the last standard one.
  const latest = $derived(latestPerSuite(runs));

  const shortName = (name: string | null) => (name ? name.replace(ZAPRET2, "") : S.overview.noStrategy);

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

  onMount(() => {
    void refreshOverview();
    void checkProxy();
  });
</script>

{#snippet pill(label: string, score: ProfileScore)}
  {#if score.total > 0}
    <span class="pill {score.ok === score.total ? 'ok' : 'warn'}">{label}: {score.ok}/{score.total} · {ms(score.median_ms)}</span>
  {/if}
{/snippet}

{#if app.overview}
  {@const o = app.overview}
  {@const locked = app.busy || o.testing}
  {@const running = o.service === "Running"}

  <div class="banner" class:active={running}>
    <div class="info">
      <span class="dot"></span>
      <div>
        <h2>{running ? S.overview.bannerActive(shortName(o.strategy)) : S.overview.bannerInactive}</h2>
        <p class="muted">
          {#if running}
            {S.overview.interceptor}: {app.linux ? S.overview.interceptorLinux : S.overview.interceptorWindows}
          {:else if o.strategy}
            {S.overview.strategy}: {shortName(o.strategy)} · {S.svc[o.service]}
          {:else}
            {S.overview.notRunning}
          {/if}
        </p>
      </div>
    </div>
    <div class="buttons">
      {#if running}
        <button class="stop" disabled={locked} onclick={() => change(S.failed.stopService, () => commands.stopService())}>
          {S.overview.stopBypass}
        </button>
        <button disabled={locked} onclick={() => app.setTab("strategies")}>{S.overview.changeStrategy}</button>
      {:else if o.strategy}
        {@const strategy = o.strategy}
        <button
          class="primary"
          disabled={locked}
          onclick={() =>
            o.service === "Stopped"
              ? change(S.failed.startService, () => commands.startService())
              : change(S.failed.apply, () => commands.applyStrategy(strategy))}
        >
          {S.overview.enableBypass}
        </button>
        <button disabled={locked} onclick={() => app.setTab("tests")}>{S.overview.pickStrategy}</button>
      {:else if o.last_run?.best}
        <button class="primary" disabled={locked} onclick={() => change(S.failed.apply, () => commands.applyBest())}>
          {S.overview.applyBest}
        </button>
        <button disabled={locked} onclick={() => app.setTab("tests")}>{S.overview.pickStrategy}</button>
      {:else}
        <button class="primary" disabled={locked} onclick={() => app.setTab("tests")}>{S.overview.pickStrategy}</button>
      {/if}
    </div>
  </div>

  <div class="cards">
    <section class="card engine">
      <h2>{S.overview.engineState}</h2>
      <div class="rows">
        <div><span class="muted">{S.overview.service(o.service_name)}:</span> <span class={running ? "ok" : "bad"}>{S.svc[o.service]}</span></div>
        <div>
          <span class="muted">{S.overview.engineVersion}:</span>
          {#if !o.zapret2.dir}<span class="warn">{S.overview.zapret2Off}</span>{:else}{S.overview.version(o.zapret2.version)}{/if}
        </div>
        <div><span class="muted">{S.overview.strategy}:</span> <strong>{shortName(o.strategy)}</strong></div>
      </div>
      {#if !o.zapret2.dir}
        <p class="muted">{S.overview.zapret2OffHint}</p>
        <div class="buttons"><button onclick={() => app.setTab("settings")}>{S.overview.zapret2Connect}</button></div>
      {:else if o.zapret2.error}
        <pre class="bad">{o.zapret2.error}</pre>
      {/if}
      {#if running && !o.winws_running}<p class="warn">{S.overview.winwsMissing}</p>{/if}
      {#if o.service === "StopPending" || o.windivert === "StopPending"}<p class="warn">{S.overview.stopPending}</p>{/if}

      <div class="quick">
        <p>
          <button class="link" title={S.overview.quickWhere} onclick={() => app.setTab("diag")}>{S.overview.quick}</button>:
          {#if o.quick}
            <span class={o.quick.ok ? "ok" : "bad"}>{o.quick.ok ? S.overview.quickOk : S.overview.quickFailed}</span>
          {:else}
            <span class="muted">{S.overview.quickNever}</span>
          {/if}
        </p>
        {#if o.quick}
          <div class="pills">
            {#each o.quick.checks as item (item.name)}
              <span class="pill">
                {item.name}: <strong class={item.status === "Ok" ? "ok" : "bad"}>{item.status === "Ok" ? ms(item.time_ms) : S.status[item.status]}</strong>
              </span>
            {/each}
          </div>
        {/if}
      </div>

      <div class="buttons">
        <button disabled={locked || !running} onclick={() => change(S.failed.quick, () => commands.quickCheck())}>{S.overview.quick}</button>
        <button disabled={locked} onclick={() => change(S.failed.restart, () => commands.restartZapret())}>{S.overview.restart}</button>
      </div>
    </section>

    <section class="card">
      <h2>{S.overview.lastTest}</h2>
      {#each latest as run (run.id)}
        <h3>{run.suite === "Standard" ? S.tests.standard : S.tests.dpi}</h3>
        {#if run.best && run.best_score}
          {@const score = run.best_score}
          <p>{S.overview.best}: <strong>{shortName(run.best)}</strong></p>
          <p>
            {run.suite === "Standard"
              ? S.overview.errors(score.total - score.ok, score.total, ms(score.median_ms))
              : S.strategies.dpiScore(score.likely_blocked, score.fail)}
          </p>
          <div class="pills">
            {#if run.suite === "Standard"}
              {@render pill(S.results.discord, score.discord)}
              {@render pill(S.results.youtube, score.youtube)}
              {@render pill(S.results.ping, { ok: score.ping_ok, total: score.ping_total, median_ms: score.ping_median_ms })}
            {:else}
              <span class="pill {score.ok === score.total ? 'ok' : 'warn'}">{S.overview.scoreOk(score.ok, score.total)}</span>
            {/if}
          </div>
        {/if}
        <p class="muted">{runDate(run.id)}</p>
      {:else}
        <p class="muted">{S.overview.noTests}</p>
      {/each}
      <div class="buttons">
        <button class="primary" disabled={locked || !o.last_run?.best} onclick={() => change(S.failed.apply, () => commands.applyBest())}>
          {S.overview.applyBest}
        </button>
        <button disabled={o.testing} onclick={() => app.setTab("tests")}>{S.overview.runTests}</button>
      </div>
    </section>

    <TgWsCard />

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
  </div>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<style>
  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 16px 20px;
    border-radius: 10px;
    background: var(--panel);
    border: 1px solid var(--border);
  }

  .banner.active {
    background: color-mix(in srgb, var(--ok) 7%, var(--panel));
    border-color: color-mix(in srgb, var(--ok) 22%, var(--border));
  }

  .info {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  /* A calm matte dot, not a glowing one. */
  .dot {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    border-radius: 50%;
    background: color-mix(in srgb, var(--bad) 40%, var(--muted));
  }

  .banner.active .dot {
    background: color-mix(in srgb, var(--ok) 55%, var(--panel));
  }

  .banner h2 {
    margin: 0;
    font-size: 17px;
  }

  .banner p {
    margin: 2px 0 0;
    font-size: 13px;
  }

  .banner .buttons {
    margin: 0;
  }

  /* A muted stop button, not a bright red frame. */
  button.stop {
    border-color: color-mix(in srgb, var(--bad) 45%, var(--border));
    color: color-mix(in srgb, var(--bad) 70%, var(--text));
  }

  @media (min-width: 1000px) {
    .cards {
      grid-template-columns: 1.5fr 1fr;
    }
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .card h2,
  .card h3,
  .card p,
  .card pre,
  .card .buttons {
    margin: 0;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .quick {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .pill {
    padding: 2px 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 12px;
  }
</style>

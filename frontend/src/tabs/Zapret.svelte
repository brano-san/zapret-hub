<script lang="ts">
  import { untrack } from "svelte";
  import { commands, type CacheReport, type CheckAction, type SystemCheck } from "$lib/ipc/bindings";
  import CopyButton from "../components/CopyButton.svelte";
  import TgWsCard from "../components/TgWsCard.svelte";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { levelClass } from "$lib/format";
  import { notices } from "$lib/notices.svelte";
  import { S } from "$lib/strings";

  const RISKY: CheckAction[] = ["RemoveWinDivert", "RemoveConflicts"];

  let checks = $state<SystemCheck[] | null>(null);
  let cache = $state<CacheReport[] | null>(null);
  let status = $state("");
  let rules = $state("");
  let rulesBusy = $state(false);
  let logs = $state("");
  let logsBusy = $state(false);
  let logsEl = $state<HTMLPreElement | null>(null);

  const locked = $derived(app.busy || (app.overview?.testing ?? false));
  const service = $derived(app.overview?.service ?? null);
  const running = $derived(service === "Running");
  // A table with rules prints a non-empty listing; anything else is no active table.
  const nftActive = $derived(rules.trim() !== "");

  // The journal is at the bottom of its box: jump to the newest lines after each load.
  $effect(() => {
    if (logs && logsEl) logsEl.scrollTop = logsEl.scrollHeight;
  });

  async function loadRules(): Promise<void> {
    rulesBusy = true;
    rules = (await call(S.failed.nftRules, commands.nftRules())) ?? rules;
    rulesBusy = false;
  }

  async function loadLogs(): Promise<void> {
    logsBusy = true;
    logs = (await call(S.failed.serviceLogs, commands.serviceLogs())) ?? logs;
    logsBusy = false;
  }

  /** The Linux monitors follow the service: read again after it changes. */
  function reload(): void {
    if (!app.linux) return;
    void loadRules();
    void loadLogs();
  }

  // The platform arrives with the overview, maybe after the tab mounts.
  $effect(() => {
    if (app.linux) untrack(reload);
  });

  async function serviceAction(title: string, run: () => ReturnType<typeof commands.startService>): Promise<void> {
    await change(title, run);
    reload();
  }

  async function stopService(): Promise<void> {
    const sure = await confirmation.ask({ title: S.zapret.stopTitle, text: S.zapret.stopText, yes: S.zapret.stop });
    if (sure) await serviceAction(S.failed.stopService, () => commands.stopService());
  }

  async function removeService(): Promise<void> {
    const sure = await confirmation.ask({
      title: S.zapret.removeTitle,
      text: `${app.linux ? S.zapret.serviceHintLinux : S.zapret.serviceHint}\n\n${S.zapret.removeText}`,
      yes: S.zapret.remove,
      danger: true,
    });
    if (sure) await serviceAction(S.failed.removeService, () => commands.removeService());
  }

  async function flushDns(): Promise<void> {
    if ((await change(S.failed.flushDns, () => commands.flushDns())) !== undefined) {
      notices.push({ level: "Info", title: S.zapret.dnsFlushed, text: "" });
    }
  }

  /** Shows what changes before anything is written (spec 5.6). */
  async function updateHosts(): Promise<void> {
    status = "";
    const plan = await change(S.failed.hosts, () => commands.hostsPlan());
    if (!plan) return;
    if (!plan.needs_update) {
      status = S.zapret.hostsCurrent;
      return;
    }
    const diff = [...plan.added.map((line) => `+ ${line}`), ...plan.removed.map((line) => `− ${line}`)].join("\n");
    const sure = await confirmation.ask({ title: S.zapret.hostsTitle, text: diff, yes: S.zapret.hostsApply });
    if (sure && (await change(S.failed.hosts, () => commands.applyHosts())) !== undefined) status = S.zapret.hostsUpdated;
  }

  async function runChecks(): Promise<void> {
    checks = (await change(S.failed.checks, () => commands.systemChecks())) ?? null;
  }

  async function fix(check: SystemCheck, action: CheckAction): Promise<void> {
    if (RISKY.includes(action)) {
      const sure = await confirmation.ask({
        title: S.checks.actions[action],
        text: S.checks.confirm(action, check.detail),
        yes: S.checks.actions[action],
        danger: true,
      });
      if (!sure) return;
    }
    // A fix that failed may have done part of its work (`RemoveConflicts` tries every service): the checks are read again either way.
    await change(S.failed.fix, () => commands.fixCheck(action));
    await runChecks();
  }

  async function clearCache(): Promise<void> {
    const sure = await confirmation.ask({ title: S.zapret.cacheTitle, text: S.zapret.cacheText, yes: S.zapret.cacheClear });
    if (sure) cache = (await change(S.failed.cache, () => commands.clearDiscordCache())) ?? null;
  }
</script>

<div class="cards">
  <section class="card">
    <h2>{S.zapret.service}</h2>
    <p>
      {S.zapret.statusLabel}:
      <span class="badge" class:ok={running} class:warn={service === "Stopped"} class:muted={service === "NotInstalled"}>
        {service ? S.svc[service] : S.common.loading}
      </span>
    </p>
    <p class="muted">{app.linux ? S.zapret.serviceHintLinux : S.zapret.serviceHint}</p>
    <div class="buttons">
      {#if service === "NotInstalled"}
        <p class="muted">{S.zapret.notInstalled}</p>
        <button onclick={() => app.setTab("strategies")}>{S.zapret.toStrategies}</button>
      {:else}
        {#if running}
          <button disabled={locked} onclick={stopService}>{S.zapret.stop}</button>
        {:else}
          <button class="primary" disabled={locked} onclick={() => serviceAction(S.failed.startService, () => commands.startService())}>
            {S.zapret.start}
          </button>
        {/if}
        <button disabled={locked} onclick={() => serviceAction(S.failed.restartService, () => commands.restartService())}>{S.zapret.restart}</button>
        <button class="danger" disabled={locked} onclick={removeService}>{S.zapret.remove}</button>
      {/if}
    </div>
  </section>

  <section class="card">
    <h2>{S.zapret.utilsTitle}</h2>
    <div class="utility">
      <button disabled={app.busy} onclick={flushDns}>{S.zapret.dnsButton}</button>
      <p class="muted">{app.linux ? S.zapret.dnsHintLinux : S.zapret.dnsHintWindows}</p>
    </div>
    <div class="utility">
      <button disabled={app.busy} onclick={clearCache}>{S.zapret.cacheButton}</button>
      <p class="muted">{S.zapret.cacheText}</p>
    </div>
    {#if cache}
      {#if cache.length === 0}<p class="muted">{S.zapret.cacheNone}</p>{/if}
      {#each cache as report (report.install)}
        <h3>{report.install} {#if report.closed}<span class="muted">{S.zapret.cacheClosed}</span>{/if}</h3>
        {#each report.dirs as dir (dir.path)}
          <p class={dir.outcome === "Failed" ? "bad" : "muted"}>
            {dir.path} — {S.zapret.cacheOutcome[dir.outcome]}{dir.error ? `: ${dir.error}` : ""}
          </p>
        {/each}
      {/each}
    {/if}
    <div class="utility">
      <button disabled={locked} onclick={updateHosts}>{S.zapret.updateHosts}</button>
      <p class="muted">{S.zapret.hostsHint}</p>
      {#if status}<p class="ok">{status}</p>{/if}
    </div>
  </section>

  <TgWsCard full />
</div>

{#if app.linux}
  <section class="card">
    <div class="head">
      <h2>
        {S.zapret.nftTitle}
        <span class="badge" class:ok={nftActive} class:muted={!nftActive}>{nftActive ? S.zapret.nftActive : S.zapret.nftInactive}</span>
      </h2>
      <button disabled={rulesBusy} onclick={loadRules}>{rulesBusy ? S.common.loading : S.zapret.refresh}</button>
    </div>
    <p class="muted">{S.zapret.nftHint}</p>
    <pre class="monitor">{rules || (rulesBusy ? S.common.loading : S.zapret.nftEmpty)}</pre>
  </section>

  <section class="card">
    <div class="head">
      <h2>{S.zapret.logsTitle}</h2>
      <div class="buttons">
        <CopyButton text={() => logs} />
        <button disabled={logsBusy} onclick={loadLogs}>{logsBusy ? S.common.loading : S.zapret.refresh}</button>
      </div>
    </div>
    <p class="muted">{S.zapret.logsHint}</p>
    <pre class="monitor logs" bind:this={logsEl}>{logs || (logsBusy ? S.common.loading : S.zapret.logsEmpty)}</pre>
  </section>
{/if}

<section class="card">
  <h2>{S.checks.title}</h2>
  <div class="buttons"><button disabled={app.busy} onclick={runChecks}>{S.checks.run}</button></div>
  {#if checks}
    {#each checks as check (check.id)}
      <div class="check">
        <span class={levelClass[check.level]}>●</span>
        <strong>{S.checks.names[check.id]}</strong>
        <span class="muted">— {S.checks.level[check.level]}</span>
        {#if check.level !== "Ok"}
          <p class="hint">{S.checks.hints[check.id]}</p>
          {#if check.detail}<pre>{check.detail}</pre>{/if}
          <div class="buttons">
            {#if S.checks.links[check.id]}
              {@const url = S.checks.links[check.id] ?? ""}
              <button class="link" onclick={() => call(S.failed.open, commands.openUrl(url))}>{S.checks.issue}</button>
            {/if}
            {#if check.action}
              {@const action = check.action}
              <button disabled={locked} onclick={() => fix(check, action)}>{S.checks.actions[action]}</button>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</section>

<style>
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

  .utility {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .head h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  pre.monitor {
    height: 240px;
    overflow-y: auto;
  }

  pre.logs {
    height: 320px;
    background: #0d1117;
    color: #3fb950;
    border-color: #30363d;
  }

  .check {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }

  /* A path in the cache report has no space to wrap at: it breaks anywhere rather than spill out of its card. */
  p {
    overflow-wrap: anywhere;
  }
</style>

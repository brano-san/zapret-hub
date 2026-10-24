<script lang="ts">
  import { commands, type CacheReport, type CheckAction, type SystemCheck } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { levelClass } from "$lib/format";
  import { S } from "$lib/strings";

  const RISKY: CheckAction[] = ["RemoveWinDivert", "RemoveConflicts"];

  let checks = $state<SystemCheck[] | null>(null);
  let cache = $state<CacheReport[] | null>(null);
  let status = $state("");

  const locked = $derived(app.busy || (app.overview?.testing ?? false));

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

  async function removeService(): Promise<void> {
    const sure = await confirmation.ask({
      title: S.zapret.removeTitle,
      text: `${S.zapret.serviceHint}\n\n${S.zapret.removeText}`,
      yes: S.zapret.remove,
      danger: true,
    });
    if (sure) await change(S.failed.removeService, () => commands.removeService());
  }
</script>

<div class="cards">
  <section class="card">
    <h2>{S.zapret.hosts}</h2>
    <p class="muted">{S.zapret.hostsHint}</p>
    <div class="buttons"><button disabled={locked} onclick={updateHosts}>{S.zapret.updateHosts}</button></div>
    {#if status}<p class="ok">{status}</p>{/if}
  </section>

  <section class="card">
    <h2>{S.zapret.service}</h2>
    <p class="muted">{S.zapret.serviceHint}</p>
    <div class="buttons"><button class="danger" disabled={locked} onclick={removeService}>{S.zapret.remove}</button></div>
  </section>

  <section class="card">
    <h2>{S.zapret.cache}</h2>
    <div class="buttons"><button disabled={app.busy} onclick={clearCache}>{S.zapret.cacheButton}</button></div>
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
  </section>
</div>

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
  .check {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }

  /* A path in the cache report has no space to wrap at: it breaks anywhere rather than spill out of its card. */
  p {
    overflow-wrap: anywhere;
  }
</style>

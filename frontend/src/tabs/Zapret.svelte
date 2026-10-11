<script lang="ts">
  import {
    commands,
    type CacheReport,
    type CheckAction,
    type FakeKind,
    type FlowsealState,
    type GameFilter,
    type GameFilterMode,
    type IpsetMode,
    type SystemCheck,
  } from "$lib/ipc/bindings";
  import { call, type Answer } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { levelClass } from "$lib/format";
  import { S } from "$lib/strings";
  import ListEditor from "../components/ListEditor.svelte";

  const MODES: GameFilterMode[] = ["disabled", "all", "tcp", "udp"];
  const IPSET: IpsetMode[] = ["loaded", "none", "any"];
  const FAKES: FakeKind[] = ["Discord", "Game"];
  const RISKY: CheckAction[] = ["RemoveWinDivert", "RemoveConflicts"];

  let flowseal = $state<FlowsealState | null>(null);
  let filter = $state<GameFilter | null>(null);
  let checks = $state<SystemCheck[] | null>(null);
  let cache = $state<CacheReport[] | null>(null);
  let status = $state("");
  /** Bumped when a change fails: a control keeps what was clicked, and only a rebuild shows what is in effect again. */
  let rebuilt = $state(0);

  const locked = $derived(app.busy || (app.overview?.testing ?? false));

  async function load(): Promise<void> {
    const next = (await call(S.failed.flowseal, commands.getFlowseal())) ?? null;
    // The draft follows the stored filter only when that changed: another change on this tab must not drop an edit not applied yet.
    if (next && JSON.stringify(next.game_filter) !== JSON.stringify(flowseal?.game_filter)) filter = { ...next.game_filter };
    flowseal = next;
  }

  // zapret2 alone: the Flowseal settings and checks have no folder to read.
  const hasFolder = $derived(Boolean(app.settings?.zapret_dir));

  $effect(() => {
    void app.revision;
    if (hasFolder) void load();
  });

  async function applyFilter(): Promise<void> {
    if (!filter) return;
    const chosen = filter;
    await change(S.failed.gameFilter, () => commands.setGameFilter(chosen));
  }

  /** `change` for a control that shows a setting: a failed one leaves the setting as it was, so the control is rebuilt. */
  async function adjust(title: string, run: () => Promise<Answer<unknown>>): Promise<void> {
    if ((await change(title, run)) === undefined) rebuilt++;
  }

  async function updateIpset(): Promise<void> {
    status = "";
    if ((await change(S.failed.updateIpset, () => commands.updateIpset())) !== undefined) status = S.zapret.ipsetUpdated;
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

{#snippet engineCards()}
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
{/snippet}

{#if !hasFolder}
  <div class="cards">
    <section class="card">
      <h2>{S.zapret.noFolder}</h2>
      <p class="muted">{S.zapret.noFolderText}</p>
      <div class="buttons"><button onclick={() => (app.tab = "settings")}>{S.overview.chooseFolder}</button></div>
    </section>
    {@render engineCards()}
  </div>
{:else if flowseal && filter}
  {@const f = flowseal}
  {#key rebuilt}
    <div class="cards">
      <section class="card">
        <h2>{S.zapret.gameFilter}</h2>
        <select bind:value={filter.mode} disabled={locked} aria-label={S.zapret.gameFilter}>
          {#each MODES as mode (mode)}<option value={mode}>{S.zapret.gameModes[mode]}</option>{/each}
        </select>
        {#if filter.mode !== "disabled"}
          <div class="ranges">
            <label for="tcp">{S.zapret.tcpRange}</label>
            <input id="tcp" bind:value={filter.tcp} disabled={locked} />
            <label for="udp">{S.zapret.udpRange}</label>
            <input id="udp" bind:value={filter.udp} disabled={locked} />
          </div>
        {/if}
        <p class="muted">{S.zapret.gameHint}</p>
        <div class="buttons"><button class="primary" disabled={locked} onclick={applyFilter}>{S.common.apply}</button></div>
      </section>

      <section class="card">
        <h2>{S.zapret.ipset}</h2>
        {#each IPSET as mode (mode)}
          <label class="choice">
            <input
              type="radio"
              name="ipset"
              checked={f.ipset === mode}
              disabled={locked || (mode === "loaded" && f.ipset !== "loaded" && !f.ipset_backup)}
              onchange={() => adjust(S.failed.ipset, () => commands.setIpsetMode(mode))}
            />
            {S.zapret.ipsetModes[mode]}
          </label>
        {/each}
        {#if f.ipset !== "loaded" && !f.ipset_backup}<p class="warn">{S.zapret.noBackup}</p>{/if}
        <p class="muted">{S.zapret.ipsetHint}</p>
      </section>

      <section class="card">
        <h2>{S.zapret.updates}</h2>
        <label class="inline">
          <input
            type="checkbox"
            checked={f.check_updates}
            disabled={locked}
            onchange={(event) => adjust(S.failed.checkUpdates, () => commands.setCheckUpdates(event.currentTarget.checked))}
          />
          {S.zapret.checkUpdates}
        </label>
        <div class="buttons">
          <button disabled={locked} onclick={updateIpset}>{S.zapret.updateIpset}</button>
          <button disabled={locked} onclick={updateHosts}>{S.zapret.updateHosts}</button>
        </div>
        {#if status}<p class="ok">{status}</p>{/if}
      </section>

      <section class="card">
        <h2>{S.zapret.fakesTitle}</h2>
        <p class="muted">{S.zapret.fakesHint}</p>
        {#each FAKES as kind (kind)}
          {@const current = kind === "Discord" ? f.fakes.discord : f.fakes.game}
          <div class="row">
            <label for={`fake-${kind}`}>{S.zapret.fakes[kind]}</label>
            <select
              id={`fake-${kind}`}
              value={current ?? ""}
              disabled={locked}
              onchange={(event) => adjust(S.failed.fake, () => commands.replaceFake(kind, event.currentTarget.value))}
            >
              {#if !current}<option value="">{S.zapret.fakeUnknown}</option>{/if}
              {#each f.fakes.files as file (file)}<option value={file}>{file}</option>{/each}
            </select>
          </div>
        {/each}
      </section>

      {@render engineCards()}
    </div>
  {/key}

  <section class="card">
    <h2>{S.zapret.listsTitle}</h2>
    <div class="cards">
      {#each f.lists as item (item.list)}
        <ListEditor list={item.list} entries={item.entries} disabled={locked} />
      {/each}
    </div>
  </section>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

{#if hasFolder}
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
{/if}

<style>
  .check {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }

  /* Two labelled fields stacked: four items in a row are wider than a card at the default window size. */
  .ranges {
    margin-top: 8px;
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 6px 8px;
  }

  .ranges input {
    width: 100%;
    min-width: 0;
  }

  /* The fake lists stand apart from each other and from the text above. */
  .row {
    margin-top: 10px;
  }

  /* The longest fake name is wider than a card between the 2- and 3-column layouts: the select gives way. */
  .row select {
    flex: 1;
    min-width: 0;
  }

  /* A path in the cache report has no space to wrap at: it breaks anywhere rather than spill out of its card. */
  p {
    overflow-wrap: anywhere;
  }
</style>

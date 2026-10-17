<script lang="ts">
  import { commands, type RunSummary, type Score, type StrategyRow, type Suite, type TestRun } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { estimateMinutes, ms, runDate, zapret2Name } from "$lib/format";
  import { S } from "$lib/strings";
  import CopyButton from "../components/CopyButton.svelte";
  import RepeatsSelect from "../components/RepeatsSelect.svelte";
  import ResultsTable from "../components/ResultsTable.svelte";

  let rows = $state<StrategyRow[]>([]);
  let suite = $state<Suite>("Standard");
  let repeats = $state(1);
  let chosen = $state<string[]>([]);
  let runs = $state<RunSummary[]>([]);
  let shown = $state<TestRun | null>(null);
  // The selection is set once; later reloads keep what the user chose.
  let picked = false;

  const usable = (list: StrategyRow[]) => list.filter((row) => row.error === null && !row.flowseal_copy).map((row) => row.name);
  const flowseal = $derived(rows.filter((row) => zapret2Name(row.name) === null));
  const zapret2 = $derived(rows.filter((row) => zapret2Name(row.name) !== null));
  // Ticks or clears one group; the other group's ticks stay.
  function setGroup(list: StrategyRow[], on: boolean): void {
    const inGroup = new Set(list.map((row) => row.name));
    chosen = [...chosen.filter((name) => !inGroup.has(name)), ...(on ? usable(list) : [])];
  }
  const suiteName = (value: Suite) => (value === "Standard" ? S.tests.standard : S.tests.dpi);

  async function openRun(id: string): Promise<void> {
    shown = (await call(S.failed.run, commands.getRun(id))) ?? null;
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    runs = (await call(S.failed.runs, commands.listRuns())) ?? [];
    if (!picked && rows.length > 0) {
      chosen = usable(rows);
      picked = true;
    }
    if (!shown && runs[0]) await openRun(runs[0].id);
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  $effect(() => {
    const id = app.finished;
    if (id) {
      app.finished = null;
      void openRun(id);
    }
  });

  async function start(): Promise<void> {
    // In the menu's order, whatever order they were ticked in.
    const strategies = rows.filter((row) => chosen.includes(row.name)).map((row) => row.name);
    if ((await change(S.failed.startTests, () => commands.startTests({ suite, strategies, repeats }))) !== undefined) {
      await refreshLive();
    }
  }

  function apply(name: string): void {
    void change(S.failed.apply, () => commands.applyStrategy(name));
  }
</script>

{#if app.live}
  {@const live = app.live}
  {@const total = live.request.strategies.length}
  {@const passes = live.request.repeats ?? 1}
  <section class="card">
    <h2>{suiteName(live.request.suite)}</h2>
    {#if live.restoring}
      <p>{S.tests.restoring}</p>
    {:else if live.current !== null}
      <p>{S.tests.progress(live.current + 1, total, live.request.strategies[live.current] ?? "", live.pass + 1, passes)}</p>
    {:else}
      <p>{S.tests.preparing}</p>
    {/if}
    <progress class="wide" max={total * passes} value={live.restoring ? total * passes : live.pass * total + (live.current ?? 0)}></progress>
    <div class="buttons">
      <button class="danger" disabled={live.restoring} onclick={() => commands.cancelTests()}>{S.common.cancel}</button>
    </div>
  </section>
  <section class="card">
    <ResultsTable suite={live.request.suite} results={live.results} />
  </section>
{:else}
  <section class="card">
    <h2>{S.tests.newRun}</h2>
    <div class="row">
      <label class="inline"><input type="radio" bind:group={suite} value="Standard" /> {S.tests.standard}</label>
      <label class="inline"><input type="radio" bind:group={suite} value="Dpi" /> {S.tests.dpi}</label>
    </div>
    <p class="muted">{suite === "Standard" ? S.tests.standardHint : S.tests.dpiHint}</p>
    {#if app.settings?.zapret_dir}{@render group(S.tests.groupFlowseal, flowseal)}{/if}
    {#if zapret2.length > 0}{@render group(S.tests.groupZapret2, zapret2)}{/if}
    <div class="buttons">
      <button class="primary" disabled={app.busy || chosen.length === 0} onclick={start}>{S.tests.start}</button>
      <RepeatsSelect bind:value={repeats} />
      <span class="muted">{S.tests.estimate(chosen.length, estimateMinutes(chosen.length, suite, repeats), repeats)}</span>
    </div>
  </section>

  {#if shown}
    {@const run = shown}
    {@const first = run.strategies[0]}
    <section class="card">
      <h2>{S.tests.runTitle(runDate(run.id), suiteName(run.suite))}</h2>
      <p class="muted">
        {S.tests.versions(run.flowseal_version, run.zapret2_version)}{#if run.canceled}{" · "}{S.tests.canceled}{/if}
      </p>
      {#if run.restore_error}<pre class="bad">{run.restore_error}</pre>{/if}
      <div class="buttons">
        {#if first && first.launch_error === null}
          <button class="primary" disabled={app.busy} onclick={() => apply(first.name)}>{S.tests.applyBest(first.name)}</button>
        {/if}
        <CopyButton text={() => call(S.failed.report, commands.runReport(run.id))} />
      </div>
      <!-- A run opens in its ranking: another run's sort (maybe a column this suite lacks) and open row do not carry over. -->
      {#key run.id}
        <ResultsTable suite={run.suite} results={run.strategies} onApply={apply} disabled={app.busy} />
      {/key}
    </section>
  {/if}
{/if}

<section class="card">
  <h2>{S.tests.history}</h2>
  {#if runs.length === 0}<p class="muted">{S.tests.noRuns}</p>{/if}
  <table>
    <tbody>
      {#each runs as summary (summary.id)}
        <tr class:selected={shown?.id === summary.id}>
          <td><button class="link" onclick={() => openRun(summary.id)}>{runDate(summary.id)}</button></td>
          <td>{suiteName(summary.suite)}{#if summary.canceled}{" · "}{S.tests.canceled}{/if}</td>
          <td>{S.tests.runCount(summary.strategies)}</td>
          <td>{summary.best ?? "—"}</td>
          <td>{#if summary.best_score}{@render score(summary.suite, summary.best_score)}{/if}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

{#snippet group(title: string, list: StrategyRow[])}
  <div class="group">
    <h3>{title}</h3>
    <button onclick={() => setGroup(list, true)}>{S.tests.all}</button>
    <button onclick={() => setGroup(list, false)}>{S.tests.none}</button>
  </div>
  <div class="choices">
    {#each list as row (row.name)}
      <label class="inline">
        <input type="checkbox" bind:group={chosen} value={row.name} disabled={row.error !== null} />
        <span>{zapret2Name(row.name) ?? row.name}</span>
      </label>
    {/each}
  </div>
{/snippet}

{#snippet score(kind: Suite, value: Score)}
  {kind === "Standard"
    ? S.tests.runScore(value.total - value.ok, value.total, ms(value.median_ms))
    : S.strategies.dpiScore(value.likely_blocked, value.fail)}
{/snippet}

<style>
  /* The results table sits in its own component: it follows the buttons above it with the usual gap. */
  .card :global(.buttons + table) {
    margin-top: 10px;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }

  .group h3 {
    margin: 0 8px 0 0;
  }

  /* Checkbox and name in columns of one width, whatever the name's length. */
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 2px 14px;
    margin-top: 8px;
  }

  .choices label.inline {
    margin: 0;
    min-width: 0;
  }

  .choices input {
    flex: none;
  }

  .choices span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

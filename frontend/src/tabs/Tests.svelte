<script lang="ts">
  import { commands, type RunSummary, type Score, type StrategyRow, type Suite, type TestRun } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { estimateMinutes, ms, runDate } from "$lib/format";
  import { S } from "$lib/strings";
  import ResultsTable from "../components/ResultsTable.svelte";

  let rows = $state<StrategyRow[]>([]);
  let suite = $state<Suite>("Standard");
  let chosen = $state<string[]>([]);
  let runs = $state<RunSummary[]>([]);
  let shown = $state<TestRun | null>(null);
  let copied = $state(false);
  // The selection is set once; later reloads keep what the user chose.
  let picked = false;

  const usable = (list: StrategyRow[]) => list.filter((row) => row.error === null).map((row) => row.name);
  const suiteName = (value: Suite) => (value === "Standard" ? S.tests.standard : S.tests.dpi);

  async function openRun(id: string): Promise<void> {
    shown = (await call(S.failed.run, commands.getRun(id))) ?? null;
    copied = false;
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
    if ((await change(S.failed.startTests, () => commands.startTests({ suite, strategies }))) !== undefined) {
      await refreshLive();
    }
  }

  async function copy(id: string): Promise<void> {
    const text = await call(S.failed.report, commands.runReport(id));
    if (text === undefined) return;
    await navigator.clipboard.writeText(text);
    copied = true;
  }

  function apply(name: string): void {
    void change(S.failed.apply, () => commands.applyStrategy(name));
  }
</script>

{#if app.live}
  {@const live = app.live}
  {@const total = live.request.strategies.length}
  <section class="card">
    <h2>{suiteName(live.request.suite)}</h2>
    {#if live.restoring}
      <p>{S.tests.restoring}</p>
    {:else if live.current !== null}
      <p>{S.tests.progress(live.current + 1, total, live.request.strategies[live.current] ?? "")}</p>
    {:else}
      <p>{S.tests.preparing}</p>
    {/if}
    <progress class="wide" max={total} value={live.results.length}></progress>
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
    <div class="buttons">
      <button onclick={() => (chosen = usable(rows))}>{S.tests.all}</button>
      <button onclick={() => (chosen = [])}>{S.tests.none}</button>
    </div>
    <div class="choices">
      {#each rows as row (row.name)}
        <label class="inline">
          <input type="checkbox" bind:group={chosen} value={row.name} disabled={row.error !== null} />
          {row.name}
        </label>
      {/each}
    </div>
    <div class="buttons">
      <button class="primary" disabled={app.busy || chosen.length === 0} onclick={start}>{S.tests.start}</button>
      <span class="muted">{S.tests.estimate(chosen.length, estimateMinutes(chosen.length, suite))}</span>
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
        <button onclick={() => copy(run.id)}>{S.common.copy}</button>
        {#if copied}<span class="ok">{S.tests.copied}</span>{/if}
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

{#snippet score(kind: Suite, value: Score)}
  {kind === "Standard"
    ? S.tests.runScore(value.total - value.ok, value.total, ms(value.median_ms))
    : S.strategies.dpiScore(value.likely_blocked, value.fail)}
{/snippet}

<style>
  .choices {
    columns: 3 220px;
    margin-top: 8px;
  }
</style>

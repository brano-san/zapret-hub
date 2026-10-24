<script lang="ts">
  import { commands, type RunSummary, type Score, type StrategyRow, type Suite, type TestRun } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { defaultTestable, estimateMinutes, ms, runDate, zapret2Name } from "$lib/format";
  import { S } from "$lib/strings";
  import CopyButton from "../components/CopyButton.svelte";
  import RepeatsSelect from "../components/RepeatsSelect.svelte";
  import ResultsTable from "../components/ResultsTable.svelte";

  let rows = $state<StrategyRow[]>([]);
  let suite = $state<Suite>("Standard");
  let repeats = $state(1);
  let chosen = $state<string[]>([]);
  let search = $state("");
  let showGames = $state(false);
  let runs = $state<RunSummary[]>([]);
  let shown = $state<TestRun | null>(null);
  let showAllHistory = $state(false);
  // The selection is set once; later reloads keep what the user chose.
  let picked = false;

  const HISTORY_LIMIT = 10;
  /** The strategy that is applied and running: a stopped one can be applied again. */
  const running = $derived(app.overview?.service === "Running" ? app.overview.strategy : null);
  const visibleRuns = $derived(showAllHistory ? runs : runs.slice(0, HISTORY_LIMIT));

  const usable = defaultTestable;
  // Ticks or clears every strategy in `list`; the ticks outside it stay.
  function setGroup(list: StrategyRow[], on: boolean): void {
    const inGroup = new Set(list.map((row) => row.name));
    chosen = [...chosen.filter((name) => !inGroup.has(name)), ...(on ? usable(list) : [])];
  }

  /** A checkbox tick without `bind:group`: the strategies the search hides keep their ticks. */
  function toggle(name: string): void {
    chosen = chosen.includes(name) ? chosen.filter((n) => n !== name) : [...chosen, name];
  }

  /** The search matches the shown name, without the `zapret2:` prefix. */
  const match = (row: StrategyRow) => (zapret2Name(row.name) ?? row.name).toLowerCase().includes(search.trim().toLowerCase());

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
    {#if rows.length > 0}
      <div class="group">
        <h3>{S.tests.group}</h3>
        <button type="button" onclick={() => setGroup(rows.filter(match), true)}>{S.tests.all}</button>
        <button type="button" onclick={() => setGroup(rows.filter(match), false)}>{S.tests.none}</button>
        <input class="search" bind:value={search} placeholder={S.tests.search} aria-label={S.tests.search} />
        <span class="muted">{S.tests.chosen(chosen.length, usable(rows).length)}</span>
      </div>
      <div class="choices">
        {#each rows.filter(match) as row (row.name)}
          <label class="inline">
            <input type="checkbox" checked={chosen.includes(row.name)} disabled={row.error !== null} onchange={() => toggle(row.name)} />
            <span>{zapret2Name(row.name) ?? row.name}</span>
          </label>
        {/each}
      </div>
    {/if}
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
        {S.tests.version(run.zapret2_version)}{#if run.canceled}{" · "}{S.tests.canceled}{/if}
      </p>
      {#if run.restore_error}<pre class="bad">{run.restore_error}</pre>{/if}
      <div class="buttons">
        {#if first && first.launch_error === null}
          {@const best = zapret2Name(first.name) ?? first.name}
          {@const bestActive = !!running && best.toLowerCase() === (zapret2Name(running) ?? running).toLowerCase()}
          <button class="primary" disabled={app.busy || bestActive} onclick={() => apply(first.name)}>
            {bestActive ? S.tests.bestActive(best) : S.tests.applyBest(best)}
          </button>
        {/if}
        <CopyButton text={() => call(S.failed.report, commands.runReport(run.id))} />
        {#if run.suite === "Standard"}
          <label class="inline"><input type="checkbox" bind:checked={showGames} /> {S.tests.games}</label>
        {/if}
      </div>
      <!-- A run opens in its ranking: another run's sort (maybe a column this suite lacks) and open row do not carry over. -->
      {#key run.id}
        <ResultsTable suite={run.suite} results={run.strategies} onApply={apply} disabled={app.busy} {showGames} activeStrategy={running} />
      {/key}
    </section>
  {/if}
{/if}

<section class="card">
  <h2>{S.tests.history}</h2>
  {#if runs.length === 0}<p class="muted">{S.tests.noRuns}</p>{/if}
  <table>
    <tbody>
      {#each visibleRuns as summary (summary.id)}
        <tr class:selected={shown?.id === summary.id}>
          <td><button class="link" onclick={() => openRun(summary.id)}>{runDate(summary.id)}</button></td>
          <td>{suiteName(summary.suite)}{#if summary.canceled}{" · "}{S.tests.canceled}{/if}</td>
          <td>{S.tests.runCount(summary.strategies)}</td>
          <td>{#if summary.best}{zapret2Name(summary.best) ?? summary.best}{:else}—{/if}</td>
          <td>{#if summary.best_score}{@render score(summary.suite, summary.best_score)}{/if}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if runs.length > HISTORY_LIMIT}
    <div class="buttons">
      <button onclick={() => (showAllHistory = !showAllHistory)}>
        {showAllHistory ? S.tests.collapseAll(HISTORY_LIMIT) : S.tests.showAll(runs.length)}
      </button>
    </div>
  {/if}
</section>

{#snippet score(kind: Suite, value: Score)}
  {kind === "Standard"
    ? S.tests.runScore(value.total - value.ok, value.total, ms(value.median_ms))
    : S.strategies.dpiScore(value.likely_blocked, value.fail)}
{/snippet}

<style>
  /* The results table sits in its own component: it follows the buttons above it with the usual gap. */
  .card :global(.buttons + .table-scroll) {
    margin-top: 14px;
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

  .group .search {
    margin-left: auto;
  }

  /* Checkbox and name in columns of one width, whatever the name's length; the scroll keeps it
  from stretching the card however many strategies there are. */
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 2px 14px;
    margin-top: 8px;
    max-height: 150px;
    overflow-y: auto;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
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

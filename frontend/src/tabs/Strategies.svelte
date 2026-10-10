<script lang="ts">
  import { commands, type StrategyDetail, type StrategyRow } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { blockSummary, hubStem, ms, runDate } from "$lib/format";
  import { S } from "$lib/strings";
  import TargetChecks from "../components/TargetChecks.svelte";
  import StrategyEditor from "./StrategyEditor.svelte";

  let rows = $state<StrategyRow[]>([]);
  let query = $state("");
  let detail = $state<StrategyDetail | null>(null);
  /** The stem of the custom strategy under edit. */
  let editing = $state<string | null>(null);
  let cloneName = $state("");

  const visible = $derived(rows.filter((row) => row.name.toLowerCase().includes(query.trim().toLowerCase())));
  const locked = $derived(app.busy || (app.overview?.testing ?? false));

  async function open(name: string): Promise<void> {
    detail = (await call(S.failed.strategy, commands.getStrategy(name))) ?? null;
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    const name = detail?.row.name;
    // A deletion announces itself before it answers: the strategy it removed must not be read again.
    if (name && rows.some((row) => row.name === name)) await open(name);
    else detail = null;
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  async function clone(source: string): Promise<void> {
    const name = cloneName.trim();
    const created = await change(S.failed.clone, () => commands.cloneStrategy(source, name));
    if (!created) return;
    cloneName = "";
    await open(hubStem(created.name));
    editing = hubStem(created.name);
  }

  async function remove(record: string, stem: string): Promise<void> {
    const sure = await confirmation.ask({
      title: S.strategies.deleteTitle,
      text: S.strategies.deleteText(stem),
      yes: S.strategies.delete,
      danger: true,
    });
    if (sure && (await change(S.failed.deleteCustom, () => commands.deleteCustom(record))) !== undefined) detail = null;
  }
</script>

<div class="split">
  <section class="card scroll">
    <input class="wide" placeholder={S.strategies.search} bind:value={query} aria-label={S.strategies.search} />
    <table>
      <thead>
        <tr><th>{S.strategies.name}</th><th>{S.strategies.result}</th></tr>
      </thead>
      <tbody>
        {#each visible as row (row.name)}
          <tr class:selected={detail?.row.name === row.name}>
            <td>
              <button
                class="link"
                onclick={() => {
                  editing = null;
                  void open(row.name);
                }}>{row.name}</button
              >
              {#if row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
              {#if row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
            </td>
            <td>{@render summary(row)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  <section class="card scroll">
    {#if editing}
      <StrategyEditor
        stem={editing}
        onClose={() => {
          editing = null;
          void load();
        }}
      />
    {:else if detail}
      {@const d = detail}
      <h2>
        {d.row.name}
        {#if d.row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
        {#if d.row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
        {#if d.row.last?.stale}<span class="badge warn">{S.strategies.stale}</span>{/if}
      </h2>
      {#if d.record}<p class="muted">{S.strategies.clonedFrom(d.record.cloned_from)}</p>{/if}
      <div class="buttons">
        <button
          class="primary"
          disabled={locked || d.row.error !== null}
          onclick={() => change(S.failed.apply, () => commands.applyStrategy(d.row.name))}>{S.common.apply}</button
        >
        {#if d.record}
          {@const record = d.record}
          <button disabled={locked} onclick={() => (editing = d.row.name)}>{S.strategies.edit}</button>
          <button class="danger" disabled={locked || d.row.active} onclick={() => remove(record.name, d.row.name)}>
            {S.strategies.delete}
          </button>
        {/if}
      </div>
      <div class="buttons">
        <input placeholder={S.strategies.cloneName} bind:value={cloneName} aria-label={S.strategies.cloneName} />
        <button disabled={locked || !cloneName.trim() || d.row.error !== null} onclick={() => clone(d.row.name)}>
          {S.strategies.clone}
        </button>
      </div>
      {#if d.row.error}<pre class="bad">{d.row.error}</pre>{/if}
      {#if d.wf}
        <h3>{S.strategies.filter}</h3>
        <pre>{d.wf}</pre>
      {/if}
      {#each d.blocks as block, i (i)}
        <h3>{S.strategies.block(i + 1)} <span class="muted">· {blockSummary(block)}</span></h3>
        <pre>{block}</pre>
      {/each}
      {#if d.final_args}
        <details>
          <summary>{S.strategies.finalArgs}</summary>
          <pre>{d.final_args}</pre>
        </details>
      {/if}
      {#if d.result}
        <h3>{S.strategies.lastRun(runDate(d.row.last?.run_id ?? ""))}</h3>
        <TargetChecks result={d.result} />
      {/if}
    {:else}
      <p class="muted">{S.strategies.pick}</p>
    {/if}
  </section>
</div>

{#snippet summary(row: StrategyRow)}
  {#if row.error}
    <span class="bad">{S.strategies.notStrategy}</span>
  {:else if !row.last}
    <span class="muted">{S.strategies.untested}</span>
  {:else if row.last.launch_error}
    <span class="bad">{S.strategies.launchError}</span>
  {:else}
    {@const score = row.last.score}
    <span class={score.total - score.ok === 0 ? "ok" : "warn"}>
      {row.last.suite === "Standard"
        ? S.strategies.score(score.total - score.ok, score.total, ms(score.median_ms))
        : S.strategies.dpiScore(score.likely_blocked, score.fail)}
    </span>
    {#if row.last.stale}<span class="badge warn">{S.strategies.stale}</span>{/if}
  {/if}
{/snippet}

<style>
  .split {
    display: grid;
    grid-template-columns: minmax(340px, 2fr) 3fr;
    gap: 14px;
    min-height: 0;
    flex: 1;
  }

  .scroll {
    overflow: auto;
  }

  /* The list gives way before it scrolls sideways: at the minimum window the result column would be cut off. */
  td {
    white-space: normal;
  }
</style>

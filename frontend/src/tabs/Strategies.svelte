<script lang="ts">
  import { commands, type Merged, type StrategyDetail, type StrategyRow } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { ZAPRET2_TEMPLATE, copyStem, blockSummary, hubStem, mergeNote, ms, nameError, ownName, runDate, uniqueName, variantLabel, zapret2Name } from "$lib/format";
  import { S } from "$lib/strings";
  import RepeatsSelect from "../components/RepeatsSelect.svelte";
  import TargetChecks from "../components/TargetChecks.svelte";
  import StrategyEditor from "./StrategyEditor.svelte";
  import Zapret2Editor from "./Zapret2Editor.svelte";

  let rows = $state<StrategyRow[]>([]);
  let query = $state("");
  let detail = $state<StrategyDetail | null>(null);
  /** The stem of the custom strategy under edit. */
  let editing = $state<string | null>(null);
  /** The zapret2 strategy under edit, or a copy not saved yet (zapret2 spec 5.2). */
  let editing2 = $state<{ name: string; from: string; create: boolean; initial?: string } | null>(null);
  /** The «new strategy» form: its engine and name. */
  let creating = $state<{ engine: "flowseal" | "zapret2"; name: string } | null>(null);
  let createError = $state("");
  /** «Сделать circular из стратегий»: the strategies picked, in order, the new name and the preview. */
  let merging = $state<{ picked: string[]; name: string; result: Merged | null } | null>(null);
  let cloneName = $state("");
  let repeats = $state(1);

  const visible = $derived(rows.filter((row) => row.name.toLowerCase().includes(query.trim().toLowerCase())));
  const locked = $derived(app.busy || (app.overview?.testing ?? false));
  /** Names a new one must not take: the user's own in either engine (and Flowseal's, harmlessly). */
  const taken = $derived(rows.map((row) => ownName(row.name)));
  const cloneError = $derived(nameError(cloneName));
  /** A run is going: another cannot start. */
  const testing = $derived((app.overview?.testing ?? false) || app.live !== null);
  /** What a merge can take: the zapret2 strategies that pass the static check. */
  const mergeable = $derived(rows.filter((row) => zapret2Name(row.name) !== null && row.error === null));

  async function open(name: string): Promise<void> {
    // Another strategy: its copy gets a fresh name; a reload of the same one keeps what was typed.
    if (detail?.row.name !== name) cloneName = uniqueName(`${copyStem(name)} copy`, taken);
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
    if (zapret2Name(source) !== null) {
      // A zapret2 copy is saved from its editor: the check may want its text changed first.
      cloneName = "";
      editing2 = { name, from: source, create: true };
      return;
    }
    const created = await change(S.failed.clone, () => commands.cloneStrategy(source, name));
    if (!created) return;
    cloneName = "";
    await open(hubStem(created.name));
    editing = hubStem(created.name);
  }

  async function create(): Promise<void> {
    if (!creating) return;
    const name = creating.name.trim();
    createError = nameError(name) ?? "";
    if (createError) return;
    if (creating.engine === "zapret2") {
      creating = null;
      editing2 = { name, from: "", create: true, initial: ZAPRET2_TEMPLATE };
      return;
    }
    // The custom-strategy model starts from a source: the plain `general`.
    const source = rows.find((row) => row.name === "general" && row.error === null);
    if (!source) {
      createError = S.strategies.newNoGeneral;
      return;
    }
    const created = await change(S.failed.clone, () => commands.cloneStrategy(source.name, name));
    if (!created) return;
    creating = null;
    await open(hubStem(created.name));
    editing = hubStem(created.name);
  }

  /** A standard run of this one strategy; its progress is on the «Тесты» tab, as for any run. */
  async function test(name: string): Promise<void> {
    const started = await change(S.failed.startTests, () => commands.startTests({ suite: "Standard", strategies: [name], repeats }));
    if (started === undefined) return;
    await refreshLive();
    app.tab = "tests";
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

  async function preview(): Promise<void> {
    if (!merging) return;
    const picked = [...merging.picked];
    const result = await call(S.failed.merge, commands.zapret2Merge(picked));
    if (merging && result) merging.result = result;
  }

  async function removeZapret2(name: string): Promise<void> {
    const sure = await confirmation.ask({
      title: S.strategies.deleteTitle,
      text: S.zapret2.deleteText(name),
      yes: S.strategies.delete,
      danger: true,
    });
    if (sure && (await change(S.failed.deleteCustom, () => commands.deleteZapret2(name))) !== undefined) detail = null;
  }
</script>

<div class="split">
  <section class="card scroll">
    {#if app.overview && !app.overview.zapret2.dir}
      <p class="muted">
        {S.strategies.zapret2Off}
        <button class="link" onclick={() => (app.tab = "settings")}>{S.overview.zapret2Connect}</button>
      </p>
    {/if}
    <div class="row search">
      <input class="grow" placeholder={S.strategies.search} bind:value={query} aria-label={S.strategies.search} />
      <button
        disabled={locked}
        onclick={() => {
          detail = null;
          editing = null;
          editing2 = null;
          merging = null;
          createError = "";
          creating = { engine: app.settings?.zapret_dir ? "flowseal" : "zapret2", name: uniqueName("my strategy", taken) };
        }}>{S.strategies.newStrategy}</button
      >
    </div>
    <div class="buttons folders">
      <button disabled={!app.settings?.zapret_dir} onclick={() => call(S.failed.open, commands.openFolder("Zapret"))}>
        {S.strategies.folderFlowseal}
      </button>
      {#if app.overview?.zapret2.dir}
        <button onclick={() => call(S.failed.open, commands.openFolder("Zapret2"))}>{S.strategies.folderZapret2}</button>
        <button
          disabled={locked}
          onclick={() => {
            detail = null;
            editing = null;
            editing2 = null;
            creating = null;
            merging = { picked: [], name: uniqueName("circular", taken), result: null };
          }}>{S.circular.merge}</button
        >
      {/if}
    </div>
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
                  editing2 = null;
                  creating = null;
                  merging = null;
                  void open(row.name);
                }}>{row.name}</button
              >
              {#if row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
              {#if row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
              {#if zapret2Name(row.name) !== null}<span class="badge muted">{S.strategies.zapret2}</span>{/if}
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
    {:else if editing2}
      <Zapret2Editor
        name={editing2.name}
        from={editing2.from}
        initial={editing2.initial}
        create={editing2.create}
        onClose={(saved) => {
          editing2 = null;
          void (saved ? open(saved) : load());
        }}
      />
    {:else if merging}
      {@const m = merging}
      <h2>{S.circular.mergeTitle}</h2>
      <p class="muted">{S.circular.mergeHint}</p>
      {#each mergeable as row (row.name)}
        {@const at = m.picked.indexOf(row.name)}
        <label class="choice">
          <input
            type="checkbox"
            checked={at >= 0}
            disabled={at < 0 && m.picked.length >= 5}
            onchange={() => {
              m.picked = at >= 0 ? m.picked.filter((name) => name !== row.name) : [...m.picked, row.name];
              m.result = null;
            }}
          />
          <span>{row.name}</span>
          {#if at >= 0}<span class="badge muted">{at + 1}</span>{/if}
        </label>
      {/each}
      <p class="muted">{S.circular.mergePick(m.picked.length)}</p>
      <div class="buttons">
        <button class="primary" disabled={app.busy || m.picked.length < 2} onclick={preview}>{S.circular.preview}</button>
        <button onclick={() => (merging = null)}>{S.common.cancel}</button>
      </div>
      {#if m.result}
        {@const result = m.result}
        {@const first = zapret2Name(m.picked[0] ?? "") ?? ""}
        <section class="part">
          <h3>{S.circular.previewTitle}</h3>
          {#if !result.notes.some((note) => note.kind === "Rotates")}<p class="warn">{S.circular.nothingRotates}</p>{/if}
          <ul class="notes">
            {#each result.notes as note, i (i)}
              <li class={note.kind === "Rotates" ? "ok" : note.kind === "Same" ? "muted" : "warn"}>{mergeNote(note, first)}</li>
            {/each}
          </ul>
          <details>
            <summary>{S.circular.previewText}</summary>
            <pre>{result.text}</pre>
          </details>
          <div class="buttons">
            <input placeholder={S.strategies.newName} bind:value={m.name} aria-label={S.strategies.newName} />
            <button
              class="primary"
              disabled={nameError(m.name) !== null}
              onclick={() => {
                editing2 = { name: m.name.trim(), from: "", create: true, initial: result.text };
                merging = null;
              }}>{S.circular.openEditor}</button
            >
          </div>
          {#if nameError(m.name)}<p class="bad">{nameError(m.name)}</p>{/if}
        </section>
      {/if}
    {:else if creating}
      <h2>{S.strategies.newTitle}</h2>
      <div class="row">
        {#if app.settings?.zapret_dir}
          <label class="inline"><input type="radio" bind:group={creating.engine} value="flowseal" /> {S.strategies.engineFlowseal}</label>
        {/if}
        {#if app.overview?.zapret2.dir}
          <label class="inline"><input type="radio" bind:group={creating.engine} value="zapret2" /> {S.strategies.engineZapret2}</label>
        {/if}
      </div>
      <p class="muted">{creating.engine === "flowseal" ? S.strategies.newFlowsealNote : S.strategies.newZapret2Note}</p>
      <div class="buttons">
        <input placeholder={S.strategies.newName} bind:value={creating.name} aria-label={S.strategies.newName} />
        <button class="primary" disabled={locked || nameError(creating.name) !== null} onclick={create}>{S.strategies.newCreate}</button>
        <button onclick={() => (creating = null)}>{S.common.cancel}</button>
      </div>
      {#if nameError(creating.name)}<p class="bad">{nameError(creating.name)}</p>{:else if createError}<p class="bad">{createError}</p>{/if}
    {:else if detail}
      {@const d = detail}
      {@const z2 = zapret2Name(d.row.name)}
      <h2>
        {d.row.name}
        {#if d.row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
        {#if d.row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
        {#if z2 !== null}<span class="badge muted">{S.strategies.zapret2}</span>{/if}
        {#if d.row.last?.stale}<span class="badge warn">{S.strategies.stale}</span>{/if}
      </h2>
      {#if d.row.error}<pre class="bad">{d.row.error}</pre>{/if}
      {#if d.wf || d.blocks.length > 0}
        <section class="part">
          <h3>{S.strategies.filtersTitle}</h3>
          {#if d.wf}
            <h4>{z2 === null ? S.strategies.filter : S.zapret2.global}</h4>
            <pre>{d.wf}</pre>
          {/if}
          {#each d.blocks as block, i (i)}
            <h4>
              {z2 === null ? S.strategies.block(i + 1) : S.zapret2.profile(i + 1)}
              <span class="muted">· {blockSummary(block)}</span>
            </h4>
            <pre>{block}</pre>
          {/each}
          {#if d.final_args}
            <details>
              <summary>{z2 === null ? S.strategies.finalArgs : S.zapret2.commandLine}</summary>
              <pre>{d.final_args}</pre>
            </details>
          {/if}
        </section>
      {/if}
      {#if d.circular.profiles.some((profile) => profile.circular) || d.circular.warnings.length > 0}
        <section class="part">
          <h3>{S.circular.title}</h3>
          <p class="muted">{S.circular.hint}</p>
          {#each d.circular.profiles.filter((profile) => profile.circular) as profile (profile.index)}
            {@const settings = profile.circular}
            <h4>{S.circular.profile(profile.number, blockSummary(profile.filter))}</h4>
            {#if settings}
              <p class="muted">
                {S.circular.settings(settings.fails, settings.retrans)}{#if settings.extra.length > 0}
                  · {S.circular.extra(settings.extra.join(":"))}{/if}
              </p>
            {/if}
            <ol class="variants">
              {#each profile.variants as variant (variant.number)}
                <li value={variant.number} title={variant.text}>
                  {variantLabel(variant.text)}
                  {#if variant.is_final}<span class="badge ok" title={S.circular.finalHint}>{S.circular.final}</span>{/if}
                </li>
              {/each}
            </ol>
          {/each}
          {#each d.circular.warnings as warning (warning)}<p class="warn">{warning}</p>{/each}
        </section>
      {/if}
      <section class="part">
        <h3>{S.strategies.resultsTitle}</h3>
        {#if d.result}
          <p class="muted">{S.strategies.lastRun(runDate(d.row.last?.run_id ?? ""))}</p>
          <TargetChecks result={d.result} />
        {:else}
          <p class="muted">{S.strategies.untested}</p>
        {/if}
        <div class="buttons">
          <button disabled={app.busy || testing || d.row.error !== null} onclick={() => test(d.row.name)}>
            {S.strategies.runTest}
          </button>
          <RepeatsSelect bind:value={repeats} disabled={app.busy || testing || d.row.error !== null} />
          {#if testing}
            <span class="muted">{S.strategies.runTestBusy}</span>
          {:else if d.row.error !== null}
            <span class="muted">{S.strategies.runTestError}</span>
          {/if}
        </div>
      </section>
      <section class="part">
        <h3>{S.strategies.cloneTitle}</h3>
        <div class="buttons">
          <input placeholder={S.strategies.cloneName} bind:value={cloneName} aria-label={S.strategies.cloneName} />
          <!-- A zapret2 copy starts from the text: a static check error is a reason to copy it. -->
          <button
            disabled={locked || cloneError !== null || (z2 === null && d.row.error !== null)}
            onclick={() => clone(d.row.name)}
          >
            {S.strategies.clone}
          </button>
        </div>
        {#if cloneError}<p class="bad">{cloneError}</p>{/if}
      </section>
      <section class="part">
        <h3>{S.strategies.applyTitle}</h3>
        <p>{S.strategies.engine}: {z2 === null ? S.strategies.engineFlowseal : S.strategies.engineZapret2}</p>
        <p>
          {S.strategies.file}: <code>{z2 === null ? `${d.row.name}.bat` : `strategies\${z2}.txt`}</code>
          <!-- A built-in zapret2 strategy lives in the app: no file to show. -->
          {#if z2 === null || d.row.custom}
            <button class="link" onclick={() => call(S.failed.open, commands.openFolder({ Strategy: d.row.name }))}>
              {S.strategies.showFile}
            </button>
          {/if}
        </p>
        {#if d.record}<p class="muted">{S.strategies.clonedFrom(d.record.cloned_from)}</p>{/if}
        <p>
          {S.strategies.state}:
          {#if d.row.active}<span class="ok">{S.strategies.activeNow}</span>{:else}<span class="muted">{S.strategies.notActive}</span>{/if}
        </p>
        <p>{S.strategies.result}: {@render summary(d.row)}{#if d.row.last}<span class="muted"> · {runDate(d.row.last.run_id)}</span>{/if}</p>
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
          {#if z2 !== null && d.row.custom}
            {@const own = z2}
            <button disabled={locked} onclick={() => (editing2 = { name: own, from: d.row.name, create: false })}>
              {S.strategies.edit}
            </button>
            <button class="danger" disabled={locked || d.row.active} onclick={() => removeZapret2(own)}>
              {S.strategies.delete}
            </button>
          {/if}
        </div>
      </section>
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
  /* The detail's blocks: filters, results, clone and apply each stand in a frame of their own. */
  .part {
    margin-top: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .part h3 {
    margin-top: 0;
  }

  .part h4 {
    margin: 8px 0 0;
    font-size: 13px;
    font-weight: 600;
  }

  .variants,
  .notes {
    margin: 4px 0;
    padding-left: 22px;
  }

  .split {
    display: grid;
    grid-template-columns: minmax(340px, 2fr) 3fr;
    gap: 14px;
    min-height: 0;
    flex: 1;
  }

  .search {
    margin-bottom: 10px;
  }

  .folders {
    margin: 0 0 10px;
  }

  .scroll {
    overflow: auto;
  }

  /* The list gives way before it scrolls sideways: at the minimum window the result column would be cut off. */
  td {
    white-space: normal;
  }
</style>

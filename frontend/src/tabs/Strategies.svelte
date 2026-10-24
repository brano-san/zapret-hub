<script lang="ts">
  import { commands, type Merged, type StrategyDetail, type StrategyRow } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { ZAPRET2_TEMPLATE, copyStem, blockSummary, mergeNote, ms, nameError, ownName, runDate, uniqueName, variantLabel, zapret2Name } from "$lib/format";
  import { detectTechniques, summarizeServices, techniqueColor, TECHNIQUES, type Technique } from "$lib/techniques";
  import { S } from "$lib/strings";
  import RepeatsSelect from "../components/RepeatsSelect.svelte";
  import TargetChecks from "../components/TargetChecks.svelte";
  import Zapret2Editor from "./Zapret2Editor.svelte";

  let rows = $state<StrategyRow[]>([]);
  let query = $state("");
  let detail = $state<StrategyDetail | null>(null);
  /** The zapret2 strategy under edit, or a copy not saved yet (zapret2 spec 5.2). */
  let editing2 = $state<{ name: string; from: string; create: boolean; initial?: string } | null>(null);
  /** The «new strategy» form: its name. */
  let creating = $state<{ name: string } | null>(null);
  let createError = $state("");
  /** «Сделать circular из стратегий»: the strategies picked, in order, the new name and the preview. */
  let merging = $state<{ picked: string[]; name: string; result: Merged | null } | null>(null);
  let cloneName = $state("");
  let repeats = $state(1);
  let tagsCache = $state<Record<string, Technique[]>>({});
  /** The tag filter over the list: every strategy, a technique, or the user's own. */
  let tagFilter = $state<"all" | Technique | "own">("all");

  const visible = $derived(
    rows.filter((row) => {
      if (!row.name.toLowerCase().includes(query.trim().toLowerCase())) return false;
      return tagFilter === "all" || (tagFilter === "own" ? row.custom : (tagsCache[row.name] ?? []).includes(tagFilter));
    }),
  );
  /** Times a technique appears in the list: a filter without matches does not appear. */
  const tagCount = (tag: Technique): number => rows.filter((row) => (tagsCache[row.name] ?? []).includes(tag)).length;
  /** The open card's tags, once over their changes — not again per render. */
  const techs = $derived.by(() => {
    if (!detail) return [];
    return (
      tagsCache[detail.row.name] ??
      detectTechniques((detail.source || "") + " " + detail.blocks.join(" ") + " " + (detail.final_args || ""))
    );
  });
  const locked = $derived(app.busy || (app.overview?.testing ?? false));
  /** Names a new one must not take: every name in the list, by its own name. */
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
    if (detail) {
      const text = (detail.source || "") + " " + detail.blocks.join(" ") + " " + (detail.final_args || "");
      tagsCache[name] = detectTechniques(text);
    }
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    for (const row of rows) {
      // A built-in never changes; the user's own may have been edited outside the app.
      if (row.custom || !tagsCache[row.name]) {
        void commands.getStrategy(row.name).then((ans) => {
          if (ans.status === "ok") {
            const d = ans.data;
            const text = (d.source || "") + " " + d.blocks.join(" ") + " " + (d.final_args || "");
            tagsCache[row.name] = detectTechniques(text);
          }
        });
      }
    }
    const name = detail?.row.name;
    // A deletion announces itself before it answers: the strategy it removed must not be read again.
    if (name && rows.some((row) => row.name === name)) await open(name);
    else detail = null;
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  function clone(source: string): void {
    const name = cloneName.trim();
    // A copy is saved from its editor: the check may want its text changed first.
    cloneName = "";
    editing2 = { name, from: source, create: true };
  }

  async function create(): Promise<void> {
    if (!creating) return;
    const name = creating.name.trim();
    createError = nameError(name) ?? "";
    if (createError) return;
    creating = null;
    editing2 = { name, from: "", create: true, initial: ZAPRET2_TEMPLATE };
  }

  /** A standard run of this one strategy; its progress is on the «Тесты» tab, as for any run. */
  async function test(name: string): Promise<void> {
    const started = await change(S.failed.startTests, () => commands.startTests({ suite: "Standard", strategies: [name], repeats }));
    if (started === undefined) return;
    await refreshLive();
    void app.setTab("tests");
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

  async function preview(): Promise<void> {
    if (!merging) return;
    const picked = [...merging.picked];
    const result = await call(S.failed.merge, commands.zapret2Merge(picked));
    if (merging && result) merging.result = result;
  }

  /** The splitter between the list pane and the detail pane: dragging shifts the column edge. */
  let split = $state<HTMLElement>();
  let leftW = $state(360);
  let dragging = $state(false);
  const LEFT_MIN = 260;

  function dragStart(event: PointerEvent): void {
    // Without this the drag also selects the list's text under the pointer.
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    dragging = true;
    handle.setPointerCapture(event.pointerId);
  }

  function dragMove(event: PointerEvent): void {
    if (!dragging || !split) return;
    const bounds = split.getBoundingClientRect();
    // 60% at most: the detail pane keeps its share at every window width.
    leftW = Math.round(Math.min(Math.max(event.clientX - bounds.left, LEFT_MIN), bounds.width * 0.6));
  }

  /** The arrows move the edge 16px, within the same bounds as a drag. */
  function dragKey(event: KeyboardEvent): void {
    const step = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
    if (!step || !split) return;
    event.preventDefault();
    leftW = Math.round(Math.min(Math.max(leftW + step, LEFT_MIN), split.getBoundingClientRect().width * 0.6));
  }

  function dragEnd(event: PointerEvent): void {
    const handle = event.currentTarget as HTMLElement;
    dragging = false;
    handle.releasePointerCapture(event.pointerId);
  }
</script>

<div class="split" bind:this={split} class:dragging style:grid-template-columns="{leftW}px 10px minmax(0, 1fr)">
  <section class="card scroll">
    {#if app.overview && !app.overview.zapret2.dir}
      <p class="muted">
        {S.strategies.zapret2Off}
        <button class="link" onclick={() => app.setTab("settings")}>{S.overview.zapret2Connect}</button>
      </p>
    {/if}
    <div class="row search">
      <input class="grow" placeholder={S.strategies.search} bind:value={query} aria-label={S.strategies.search} />
      <button
        disabled={locked}
        onclick={() => {
          detail = null;
          editing2 = null;
          merging = null;
          createError = "";
          creating = { name: uniqueName("my strategy", taken) };
        }}>{S.strategies.newStrategy}</button
      >
    </div>
    <div class="tagbar" role="group" aria-label={S.strategies.filtersTitle}>
      <button class="tag-filter" class:on={tagFilter === "all"} onclick={() => (tagFilter = "all")}>{S.strategies.allTags}</button>
      {#each TECHNIQUES as tech (tech)}
        {@const col = techniqueColor(tech)}
        {@const count = tagCount(tech)}
        <!-- The chosen one stays even with no matches left, so it can be switched off. -->
        {#if count > 0 || tagFilter === tech}
          <button
            class="tag-filter"
            class:on={tagFilter === tech}
            style:border-color={col.border}
            style:color={col.text}
            onclick={() => (tagFilter = tagFilter === tech ? "all" : tech)}>{tech} {count}</button
          >
        {/if}
      {/each}
      <button
        class="tag-filter"
        class:on={tagFilter === "own"}
        onclick={() => (tagFilter = tagFilter === "own" ? "all" : "own")}>{S.strategies.ownTags}</button
      >
    </div>
    <div class="buttons folders">
      {#if app.overview?.zapret2.dir}
        <button onclick={() => call(S.failed.open, commands.openFolder("Zapret2"))}>{S.strategies.folderZapret2}</button>
        <button
          disabled={locked}
          onclick={() => {
            detail = null;
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
              <div class="row">
                <button
                  class="link"
                  onclick={() => {
                    editing2 = null;
                    creating = null;
                    merging = null;
                    void open(row.name);
                  }}>{zapret2Name(row.name) ?? row.name}</button
                >
                {#if row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
                {#if row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
              </div>
            </td>
            <td>{@render summary(row, true)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  <!-- A focusable separator is a widget (ARIA): the arrows move it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="handle"
    role="separator"
    aria-orientation="vertical"
    aria-valuenow={leftW}
    aria-valuemin={LEFT_MIN}
    tabindex="0"
    onpointerdown={dragStart}
    onpointermove={dragMove}
    onpointerup={dragEnd}
    onpointercancel={dragEnd}
    onkeydown={dragKey}
  ></div>

  <section class="card scroll">
    {#if editing2}
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
      <p class="muted">{S.strategies.newZapret2Note}</p>
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
        {z2 ?? d.row.name}
        {#if d.row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
        {#if d.row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
        {#if d.row.last?.stale}<span class="badge warn" title={S.strategies.staleHint}>{S.strategies.stale}</span>{/if}
      </h2>
      {#if techs.length > 0}
        <div class="tech-tags detail-tags">
          {#each techs as tech (tech)}
            {@const col = techniqueColor(tech)}
            <span class="tech-tag" style:border-color={col.border} style:background={col.bg} style:color={col.text}>{tech}</span>
          {/each}
        </div>
      {/if}
      {#if d.row.error}<pre class="bad">{d.row.error}</pre>{/if}
      <section class="part">
        <div class="buttons">
          <button
            class="primary"
            disabled={locked || d.row.error !== null}
            onclick={() => change(S.failed.apply, () => commands.applyStrategy(d.row.name))}>{S.common.apply}</button
          >
          <span class={d.row.active ? "ok" : "muted"}>{d.row.active ? S.strategies.activeNow : S.strategies.notActive}</span>
        </div>
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

      {#if d.blocks.length > 0}
        {@const serviceSummaries = summarizeServices(d.blocks)}
        {#if serviceSummaries.length > 0}
          <section class="part">
            <h3>{S.strategies.summaryTitle}</h3>
            <div class="summary-cards">
              {#each serviceSummaries as item (item.title)}
                <div class="summary-card">
                  <div class="summary-header">
                    <span class="summary-icon">{item.icon}</span>
                    <strong class="summary-name">{item.title}</strong>
                  </div>
                  <div class="summary-details">{item.details}</div>
                </div>
              {/each}
            </div>
          </section>
        {/if}
      {/if}

      {#if d.wf || d.blocks.length > 0}
        <section class="part">
          <details class="raw-cli-details">
            <summary>
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
                <path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
              {S.strategies.showCliArgs}
            </summary>
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
              <h4>{z2 === null ? S.strategies.finalArgs : S.zapret2.commandLine}</h4>
              <pre>{d.final_args}</pre>
            {/if}
          </details>
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
          {#if d.row.last?.stale}<p class="muted">{S.strategies.staleHint}</p>{/if}
          <TargetChecks result={d.result} />
        {:else}
          <p class="muted">{S.strategies.untested}</p>
        {/if}
      </section>
      <section class="part">
        <h3>{S.strategies.cloneTitle}</h3>
        <div class="buttons">
          <input placeholder={S.strategies.cloneName} bind:value={cloneName} aria-label={S.strategies.cloneName} />
          <!-- A copy starts from the text: a static check error is a reason to copy it. -->
          <button
            disabled={locked || cloneError !== null}
            onclick={() => clone(d.row.name)}
          >
            {S.strategies.clone}
          </button>
        </div>
        {#if cloneError}<p class="bad">{cloneError}</p>{/if}
      </section>
      <section class="part">
        <h3>{S.strategies.applyTitle}</h3>
        <!-- A built-in zapret2 strategy lives in the app: no file to show. -->
        {#if z2 !== null && !d.row.custom}
          <p>{S.strategies.builtinType}</p>
        {:else}
          <p>
            {S.strategies.file}: <code>{app.linux ? "strategies/" : "strategies\\"}{z2 ?? d.row.name}.txt</code>
            <button class="link" onclick={() => call(S.failed.open, commands.openFolder({ Strategy: d.row.name }))}>
              {S.strategies.showFile}
            </button>
          </p>
        {/if}
        <p>
          {S.strategies.result}: {@render summary(d.row)}{#if d.row.last}<span class="muted"> · {runDate(d.row.last.run_id)}</span>{/if}
        </p>
        <div class="buttons">
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

{#snippet summary(row: StrategyRow, /** Left list: a stale icon instead of a text badge. */ compact = false)}
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
    {#if row.last.stale}
      <span class="badge warn mini" title={S.strategies.staleHint}>{compact ? "↻" : S.strategies.stale}</span>
    {/if}
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
    gap: 4px;
    min-height: 0;
    flex: 1;
    /* The detail card scrolls inside itself: main keeps a single scrollbar out. */
    overflow: hidden;
    min-width: 0;
  }

  .split.dragging {
    /* A drag at the border must not select the strategy text under the pointer. */
    user-select: none;
    cursor: col-resize;
  }

  /* 10px of capture around a 2px line: thin to look at, easy to grab. */
  .handle {
    cursor: col-resize;
    display: flex;
    justify-content: center;
    touch-action: none;
  }

  .handle::after {
    content: "";
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: var(--border);
  }

  .handle:hover::after,
  .split.dragging .handle::after {
    background: var(--accent);
  }

  .tagbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 12px 0;
  }

  .tag-filter {
    padding: 4px 10px;
    font-size: 12px;
    border-radius: 6px;
  }

  .tag-filter.on {
    background: var(--hover);
    font-weight: 600;
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

  .tech-tags {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tech-tags.detail-tags {
    margin: 6px 0 10px;
  }

  .tech-tag {
    display: inline-block;
    padding: 1px 6px;
    font-size: 11px;
    font-weight: 500;
    border-radius: 4px;
    border: 1px solid;
    line-height: 1.3;
  }

  .summary-cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 8px;
  }

  .summary-card {
    padding: 8px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .summary-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .summary-icon {
    font-size: 15px;
  }

  .summary-name {
    font-size: 13px;
  }

  .summary-details {
    margin-top: 4px;
    font-size: 12px;
    color: var(--muted);
  }

  .raw-cli-details {
    margin-top: 4px;
  }

  .raw-cli-details summary {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    list-style: none;
    line-height: 1;
    margin: 0;
    padding: 4px 0;
    font-weight: 600;
    color: var(--accent);
  }

  .raw-cli-details summary::-webkit-details-marker {
    display: none;
  }

  .raw-cli-details summary svg {
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }

  .raw-cli-details[open] > summary svg {
    transform: rotate(90deg);
  }

  .raw-cli-details summary:hover {
    text-decoration: underline;
  }
</style>

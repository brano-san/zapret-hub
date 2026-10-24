<script lang="ts">
  import type { ProfileScore, StrategyResult, Suite } from "$lib/ipc/bindings";
  import { latencySpread, ms, zapret2Name } from "$lib/format";
  import { sortBy, type Direction } from "$lib/sort";
  import { S } from "$lib/strings";
  import TargetChecks from "./TargetChecks.svelte";

  interface Props {
    suite: Suite;
    /** Best first for a finished run; in arrival order for a live one. */
    results: StrategyResult[];
    onApply?: (name: string) => void;
    disabled?: boolean;
    /** The active strategy (from the overview): its row shows «Применено ✓» instead of «Применить». */
    activeStrategy?: string | null;
    /** The «Доп. игры (DbD, Epic)» checkbox: shows the games' column and their rows in TargetChecks. */
    showGames?: boolean;
  }

  let { suite, results, onApply, disabled = false, activeStrategy = null, showGames = false }: Props = $props();

  type Key = "rank" | "name" | "errors" | "latency" | "ping" | "discord" | "youtube" | "steam" | "epic" | "dbd" | "blocked" | "fail" | "ok";
  interface Item {
    result: StrategyResult;
    rank: number;
  }

  const STANDARD: Key[] = ["rank", "name", "errors", "latency", "ping", "discord", "youtube", "steam", "epic", "dbd"];
  const DPI: Key[] = ["rank", "name", "blocked", "fail", "ok"];
  // A strategy that did not start sorts after every one that did.
  const WORST = Number.MAX_SAFE_INTEGER;
  const MIN_W = 48;

  let key = $state<Key>("rank");
  let dir = $state<Direction>("asc");
  let open = $state<string | null>(null);
  /** The column being resized: its header, the pointer's start and the width's start. */
  let resizing: { th: HTMLElement; startX: number; startW: number } | null = null;

  const columns = $derived((suite === "Standard" ? STANDARD : DPI).filter((column) => showGames || (column !== "epic" && column !== "dbd")));
  const failed = (part: { ok: number; total: number }) => part.total - part.ok;
  const NONE: ProfileScore = { ok: 0, total: 0, median_ms: null };
  /** Steam and Epic are scored apart since 0.9.0: an older run has none of them. */
  const games = (result: StrategyResult, game: "steam" | "epic" | "dbd"): ProfileScore => result.score[game] ?? NONE;

  function value({ result, rank }: Item): number | string | null {
    const s = result.score;
    const broken = result.launch_error !== null;
    switch (key) {
      case "rank":
        return rank;
      case "name":
        return zapret2Name(result.name) ?? result.name;
      case "errors":
        return broken ? WORST : failed(s);
      case "latency":
        return broken ? null : s.median_ms;
      case "ping":
        return s.ping_median_ms;
      case "discord":
        return s.discord.total > 0 ? failed(s.discord) : null;
      case "youtube":
        return s.youtube.total > 0 ? failed(s.youtube) : null;
      case "steam":
      case "epic":
      case "dbd": {
        const stats = games(result, key);
        return stats.total > 0 ? failed(stats) : null;
      }
      case "blocked":
        return broken ? WORST : s.likely_blocked;
      case "fail":
        return broken ? WORST : s.fail;
      case "ok":
        return broken ? null : -s.ok;
    }
  }

  const items = $derived(sortBy(results.map((result, index) => ({ result, rank: index + 1 })), value, dir));

  /** The same strategy in both spellings: with and without the `zapret2:` prefix. */
  const isActive = (name: string) =>
    (zapret2Name(name) ?? name).toLowerCase() === (zapret2Name(activeStrategy ?? "") ?? (activeStrategy ?? "")).toLowerCase();

  function sortOn(column: Key): void {
    if (key === column) {
      dir = dir === "asc" ? "desc" : "asc";
    } else {
      key = column;
      dir = "asc";
    }
  }

  function resizeStart(event: PointerEvent): void {
    event.preventDefault();
    const grip = event.currentTarget as HTMLElement;
    const th = grip.closest("th");
    if (!th) return;
    resizing = { th, startX: event.clientX, startW: th.getBoundingClientRect().width };
    grip.setPointerCapture(event.pointerId);
  }

  function resizeMove(event: PointerEvent): void {
    if (!resizing) return;
    resizing.th.style.width = `${Math.round(Math.max(MIN_W, resizing.startW + event.clientX - resizing.startX))}px`;
  }

  function resizeEnd(event: PointerEvent): void {
    const grip = event.currentTarget as HTMLElement;
    grip.releasePointerCapture(event.pointerId);
    resizing = null;
  }
</script>

<div class="table-scroll" style="overflow-x: auto;">
  <table>
    <thead>
      <tr>
        {#each columns as column (column)}
          <th class:num={column !== "name"} class:col-name={column === "name"} title={S.results.hints[column]}>
            <button class="head" onclick={() => sortOn(column)}>
              {S.results[column]}{key === column ? (dir === "asc" ? " ↑" : " ↓") : ""}
            </button>
            <span class="col-resizer" role="presentation" onpointerdown={resizeStart} onpointermove={resizeMove} onpointerup={resizeEnd} onpointercancel={resizeEnd}></span>
          </th>
        {/each}
        {#if onApply}<th class="actions"></th>{/if}
      </tr>
    </thead>
    <tbody>
      {#each items as { result, rank } (result.name)}
        <tr class:selected={open === result.name}>
          {#each columns as column (column)}
            <td class:num={column !== "name"} class:col-name={column === "name"}>{@render cell(column, result, rank)}</td>
          {/each}
          {#if onApply}
            <td class="actions">
              {#if isActive(result.name)}
                <button class="applied ok" disabled><span class="ok">{S.common.applied}</span></button>
              {:else}
                <button {disabled} onclick={() => onApply?.(result.name)}>
                  {S.common.apply}
                </button>
              {/if}
            </td>
          {/if}
        </tr>
        {#if open === result.name}
          <tr>
            <td colspan={columns.length + (onApply ? 1 : 0)}><TargetChecks {result} {showGames} /></td>
          </tr>
        {/if}
      {/each}
    </tbody>
  </table>
</div>

{#snippet cell(column: Key, result: StrategyResult, rank: number)}
  {@const s = result.score}
  {#if column === "rank"}
    {rank}
  {:else if column === "name"}
    <button class="link" onclick={() => (open = open === result.name ? null : result.name)}>{zapret2Name(result.name) ?? result.name}</button>
  {:else if result.launch_error !== null}
    {#if column === "errors" || column === "blocked"}<span class="bad">{S.results.launch}</span>{/if}
  {:else if column === "errors"}
    {@const passes = result.passes ?? []}
    <span
      class={failed(s) === 0 ? "ok" : "bad"}
      title={S.results.breakdown(s) + (passes.length > 1 ? `\n${S.results.perPass(passes.map((p) => failed(p.score)))}` : "")}
      >{failed(s)} / {s.total}</span
    >
  {:else if column === "latency"}
    {@const spread = latencySpread(result)}
    {ms(s.median_ms)}
    {#if spread}
      <span class="muted spread" title={S.results.spreadHint(result.passes?.length ?? 0, spread.min, spread.max)}
        >{S.results.spread(spread.min, spread.max)}</span
      >
    {/if}
  {:else if column === "ping"}
    <span class="pair"><span>{ms(s.ping_median_ms)}</span><span class="muted">{s.ping_ok}/{s.ping_total}</span></span>
  {:else if column === "discord"}
    {@render profile(s.discord.ok, s.discord.total)}
  {:else if column === "youtube"}
    {@render profile(s.youtube.ok, s.youtube.total)}
  {:else if column === "steam" || column === "epic" || column === "dbd"}
    {@render stat(games(result, column))}
  {:else if column === "blocked"}
    <span class={s.likely_blocked > 0 ? "bad" : "ok"}>{s.likely_blocked}</span>
  {:else if column === "fail"}
    {s.fail}
  {:else}
    {s.ok} / {s.total}
  {/if}
{/snippet}

{#snippet profile(ok: number, total: number)}
  {#if total > 0}<span class={ok === total ? "ok" : "warn"}>{ok}/{total}</span>{:else}—{/if}
{/snippet}

{#snippet stat(stats: ProfileScore)}
  {#if stats.total > 0}
    <span class={stats.ok === stats.total ? "ok" : "warn"}>{stats.ok}/{stats.total} · {ms(stats.median_ms)}</span>
  {:else}—{/if}
{/snippet}

<style>
  /* Everything is centered except the name column. */
  :global(th.num),
  :global(td.num) {
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  :global(th.col-name),
  :global(td.col-name) {
    text-align: left;
  }

  /* The column with «Применить» / «Применено ✓». */
  :global(th.actions),
  :global(td.actions) {
    text-align: center;
  }

  /* A strategy already in use: calm green, cannot be applied twice. */
  .applied {
    border: 1px solid color-mix(in srgb, var(--ok) 35%, var(--border));
    background: color-mix(in srgb, var(--ok) 8%, transparent);
    color: var(--ok);
    cursor: default;
  }

  /* The point the resizer's absolute position hangs off. */
  th {
    position: relative;
  }

  /* Two fixed-width sub-cells: the value and the count beside it line up from row to row. */
  .pair {
    display: inline-grid;
    grid-template-columns: 7ch 5ch;
    column-gap: 8px;
    justify-items: end;
  }

  /* The passes' spread under the median: smaller, so the median still reads first; centered with the cell. */
  .spread {
    display: block;
    margin: 0 auto;
    font-size: 0.85em;
  }

  .head {
    display: inline-flex;
    justify-content: center;
    align-items: center;
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-weight: 600;
  }

  th.col-name .head {
    justify-content: flex-start;
  }

  .col-resizer {
    position: absolute;
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    touch-action: none;
  }

  /* The visible divider where a header can be dragged: brightens and thickens on hover. */
  .col-resizer::before {
    content: "";
    position: absolute;
    top: 20%;
    right: 3px;
    width: 1px;
    height: 60%;
    background: var(--border);
  }

  .col-resizer:hover::before {
    right: 2px;
    width: 2px;
    background: var(--accent);
  }
</style>

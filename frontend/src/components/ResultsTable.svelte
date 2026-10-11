<script lang="ts">
  import type { StrategyResult, Suite } from "$lib/ipc/bindings";
  import { latencySpread, ms } from "$lib/format";
  import { sortBy, type Direction } from "$lib/sort";
  import { S } from "$lib/strings";
  import TargetChecks from "./TargetChecks.svelte";

  interface Props {
    suite: Suite;
    /** Best first for a finished run; in arrival order for a live one. */
    results: StrategyResult[];
    onApply?: (name: string) => void;
    disabled?: boolean;
  }

  let { suite, results, onApply, disabled = false }: Props = $props();

  type Key = "rank" | "name" | "errors" | "latency" | "ping" | "discord" | "youtube" | "dbd" | "blocked" | "fail" | "ok";
  interface Item {
    result: StrategyResult;
    rank: number;
  }

  const STANDARD: Key[] = ["rank", "name", "errors", "latency", "ping", "discord", "youtube", "dbd"];
  const DPI: Key[] = ["rank", "name", "blocked", "fail", "ok"];
  // A strategy that did not start sorts after every one that did.
  const WORST = Number.MAX_SAFE_INTEGER;

  let key = $state<Key>("rank");
  let dir = $state<Direction>("asc");
  let open = $state<string | null>(null);

  const columns = $derived(suite === "Standard" ? STANDARD : DPI);
  const failed = (part: { ok: number; total: number }) => part.total - part.ok;

  function value({ result, rank }: Item): number | string | null {
    const s = result.score;
    const broken = result.launch_error !== null;
    switch (key) {
      case "rank":
        return rank;
      case "name":
        return result.name;
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
      case "dbd":
        return s.dbd.median_ms;
      case "blocked":
        return broken ? WORST : s.likely_blocked;
      case "fail":
        return broken ? WORST : s.fail;
      case "ok":
        return broken ? null : -s.ok;
    }
  }

  const items = $derived(sortBy(results.map((result, index) => ({ result, rank: index + 1 })), value, dir));

  function sortOn(column: Key): void {
    if (key === column) {
      dir = dir === "asc" ? "desc" : "asc";
    } else {
      key = column;
      dir = "asc";
    }
  }
</script>

<table>
  <thead>
    <tr>
      {#each columns as column (column)}
        <th class:num={column !== "name"}>
          <button class="head" onclick={() => sortOn(column)}>
            {S.results[column]}{key === column ? (dir === "asc" ? " ↑" : " ↓") : ""}
          </button>
        </th>
      {/each}
      {#if onApply}<th></th>{/if}
    </tr>
  </thead>
  <tbody>
    {#each items as { result, rank } (result.name)}
      <tr class:selected={open === result.name}>
        {#each columns as column (column)}
          <td class:num={column !== "name"} title={column === "dbd" ? S.results.dbdHint : undefined}>{@render cell(column, result, rank)}</td>
        {/each}
        {#if onApply}
          <td><button {disabled} onclick={() => onApply?.(result.name)}>{S.common.apply}</button></td>
        {/if}
      </tr>
      {#if open === result.name}
        <tr>
          <td colspan={columns.length + (onApply ? 1 : 0)}><TargetChecks {result} /></td>
        </tr>
      {/if}
    {/each}
  </tbody>
</table>

{#snippet cell(column: Key, result: StrategyResult, rank: number)}
  {@const s = result.score}
  {#if column === "rank"}
    {rank}
  {:else if column === "name"}
    <button class="link" onclick={() => (open = open === result.name ? null : result.name)}>{result.name}</button>
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
  {:else if column === "dbd"}
    <span class="pair">
      <span>{ms(s.dbd.median_ms)}</span>
      <span class="bad" title={S.results.dbdFailed(failed(s.dbd), s.dbd.total)}>{failed(s.dbd) > 0 ? `−${failed(s.dbd)}` : ""}</span>
    </span>
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

<style>
  :global(th.num),
  :global(td.num) {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  /* Two fixed-width sub-cells: the value and the count beside it line up from row to row. */
  .pair {
    display: inline-grid;
    grid-template-columns: 7ch 5ch;
    column-gap: 8px;
    justify-items: end;
  }

  /* The passes' spread under the median: smaller, so the median still reads first. */
  .spread {
    display: block;
    font-size: 0.85em;
  }

  .head {
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-weight: 600;
  }
</style>

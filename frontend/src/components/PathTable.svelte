<script lang="ts">
  import type { DiagScore, PathKind } from "$lib/ipc/bindings";
  import { ms, pathLabel, scoreWorks } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** The paths of one culprit: ranked in a report, in the order tried while diagnostics run. */
    results: [PathKind, DiagScore][];
    /** Index of the best path; none while the paths are still tried. */
    best?: number | null;
  }

  let { results, best = null }: Props = $props();
</script>

<table>
  <thead>
    <tr>
      <th>{S.diag.columns.path}</th>
      <th>{S.diag.columns.works}</th>
      <th>{S.diag.columns.latency}</th>
      <th>{S.diag.columns.loss}</th>
      <th>{S.diag.columns.answer}</th>
    </tr>
  </thead>
  <tbody>
    {#each results as [path, score], i (i)}
      <tr class:selected={best === i}>
        <td>{pathLabel(path)}{#if best === i}<span class="muted"> · {S.diag.best}</span>{/if}</td>
        <td class={scoreWorks(score) ? "ok" : "bad"}>{S.diag.works(scoreWorks(score), score.ok, score.total)}</td>
        <td>{ms(score.median_ms)}</td>
        <td>{score.loss_pct === null ? "—" : `${score.loss_pct} %`}</td>
        <td>{score.answer ? S.diag.answers[score.answer] : "—"}</td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  td {
    overflow-wrap: anywhere;
  }
</style>

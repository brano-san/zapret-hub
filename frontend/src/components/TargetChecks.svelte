<script lang="ts">
  import type { StrategyResult } from "$lib/ipc/bindings";
  import { dpiClass, ms, statusClass } from "$lib/format";
  import { S } from "$lib/strings";

  let { result }: { result: StrategyResult } = $props();

  const KINDS = ["Http", "Tls12", "Tls13"] as const;
</script>

{#if result.launch_error !== null}
  <pre class="bad">{result.launch_error}</pre>
{:else if result.dpi.length > 0}
  <table>
    <thead>
      <tr>
        <th>{S.results.target}</th>
        <th>{S.results.host}</th>
        {#each KINDS as kind (kind)}<th>{S.results.kinds[kind]}</th>{/each}
      </tr>
    </thead>
    <tbody>
      {#each result.dpi as target (target.id)}
        <tr>
          <td>{target.id} <span class="muted">{target.provider} {target.country}</span></td>
          <td>{target.host}</td>
          {#each KINDS as kind (kind)}
            {@const check = target.checks.find((c) => c.kind === kind)}
            <td class={check ? dpiClass[check.status] : "muted"}>{check ? S.dpiStatus[check.status] : "—"}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
{:else}
  <table>
    <thead>
      <tr>
        <th>{S.results.target}</th>
        {#each KINDS as kind (kind)}<th>{S.results.kinds[kind]}</th>{/each}
        <th>{S.results.ping}</th>
      </tr>
    </thead>
    <tbody>
      {#each result.targets as target}
        <tr>
          <td>{target.name} {#if target.extra}<span class="muted">{S.results.extra}</span>{/if}</td>
          {#each KINDS as kind (kind)}
            {@const check = target.checks.find((c) => c.kind === kind)}
            <td class={check ? statusClass[check.status] : "muted"} title={check?.stderr ?? ""}>
              {#if check}{check.status === "Ok" ? ms(check.time_ms) : S.status[check.status]}{:else}—{/if}
            </td>
          {/each}
          <td class="muted">{ms(target.ping_ms)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

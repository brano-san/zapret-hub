<script lang="ts">
  import type { StrategyResult } from "$lib/ipc/bindings";
  import { dpiClass, ms, optionalHost, statusClass } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    result: StrategyResult;
    /** The Tests tab's «Доп. игры (DbD, Epic)» checkbox; elsewhere the game rows always show. */
    showGames?: boolean;
  }

  let { result, showGames = true }: Props = $props();

  const KINDS = ["Http", "Tls12", "Tls13"] as const;
  const MIN_W = 48;

  /** The column being resized: its header, the pointer's start and the width's start. */
  let resizing: { th: HTMLElement; startX: number; startW: number } | null = null;

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

{#snippet head(label: string)}
  <th>
    {label}
    <span class="col-resizer" role="presentation" onpointerdown={resizeStart} onpointermove={resizeMove} onpointerup={resizeEnd} onpointercancel={resizeEnd}></span>
  </th>
{/snippet}

{#if result.launch_error !== null}
  <pre class="bad">{result.launch_error}</pre>
{:else if result.dpi.length > 0}
  <div class="table-wrap">
    <table class="checks">
      <thead>
        <tr>
          {@render head(S.results.target)}
          {@render head(S.results.host)}
          {#each KINDS as kind (kind)}{@render head(S.results.kinds[kind])}{/each}
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
  </div>
{:else}
  <div class="table-wrap">
    <table class="checks">
      <thead>
        <tr>
          {@render head(S.results.target)}
          {#each KINDS as kind (kind)}{@render head(S.results.kinds[kind])}{/each}
          {@render head(S.results.ping)}
        </tr>
      </thead>
      <tbody>
        {#each result.targets.filter((target) => showGames || !optionalHost(target.url)) as target}
          <tr>
            <td>{target.name} {#if target.extra}<span class="muted">{S.results.extra}</span>{/if}</td>
            {#each KINDS as kind (kind)}
              {@const check = target.checks.find((c) => c.kind === kind)}
              {@const tls13 = kind === "Tls13" && target.extra && (check === undefined || check.status === "Unsup")}
              <td class={tls13 ? "muted" : check ? statusClass[check.status] : "muted"} title={check?.stderr ?? ""}>
                {#if !tls13 && check}{check.status === "Ok" ? ms(check.time_ms) : S.status[check.status]}{:else}—{/if}
              </td>
            {/each}
            <td class="muted">{ms(target.ping_ms)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  /* Its own scroll: the sticky header pins to the table's top edge, never a row's. `contain`
  keeps the scrolling cheap inside the card's own scroll. */
  .table-wrap {
    max-height: 400px;
    overflow: auto;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    contain: content;
  }

  .checks {
    margin: 0;
    border-collapse: separate;
    border-spacing: 0;
  }

  /* One line between cells: the right and the bottom only, the edges come from the wrap. */
  .checks th,
  .checks td {
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .checks th {
    position: sticky;
    top: 0;
    z-index: 5;
    /* Solid, no transparency and no rounding: nothing shows through. The 6px shadow up seals the
    sub-pixel line of a row passing by; the inset one is the header's own bottom edge. */
    background: var(--bg);
    box-shadow:
      0 -6px 0 var(--bg),
      inset 0 -2px 0 var(--border);
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

  /* The visible divider at each header's edge, as in the results table. */
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

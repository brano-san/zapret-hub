<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type FlowRow, type Preset, type RouterState, type RouterStatus, type RoutesView, type Rule } from "$lib/ipc/bindings";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { notices } from "$lib/notices.svelte";
  import { bytes, programName, proxyLabel, routeSummary, trafficLabel, viaLabel } from "$lib/format";
  import { S } from "$lib/strings";
  import RouteEditor from "./RouteEditor.svelte";

  let status = $state<RouterStatus | null>(null);
  let view = $state<RoutesView | null>(null);
  let flows = $state<FlowRow[]>([]);
  let all = $state(false);
  let editing = $state<{ rule: Rule; create: boolean } | null>(null);

  const on = $derived(status !== null && status.state.state !== "Off");
  const error = $derived(view?.error ?? status?.rules_error ?? null);
  const dohBlind = $derived(!!view?.system_doh && view.rules.some((r) => r.rule.enabled && r.rule.domains.length > 0));

  // Every read counts: an answer older than the latest asked (say, before «все» flipped) is dropped.
  let asked = 0;
  // One notice for a run of failed reads, not one a second.
  let failing = false;

  // Latency and flows announce nothing: read once a second while the tab is open and the window is shown.
  async function refresh(): Promise<void> {
    if (document.hidden) return;
    const mine = ++asked;
    try {
      const answer = await Promise.all([commands.routerStatus(), commands.listRoutes(), commands.routeFlows(all)]);
      failing = false;
      if (mine === asked) [status, view, flows] = answer;
    } catch (err) {
      if (!failing) notices.push({ level: "Error", title: S.failed.routes, text: String(err) });
      failing = true;
    }
  }

  onMount(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let alive = true;
    // The next read starts a second after the last answered: a slow `listRoutes` does not pile them up.
    const tick = async (): Promise<void> => {
      try {
        await refresh();
      } finally {
        if (alive) timer = setTimeout(tick, 1000);
      }
    };
    void tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  });

  function stateLabel(state: RouterState): { text: string; class: string } {
    switch (state.state) {
      case "Off":
        return { text: S.routes.off, class: "muted" };
      case "Running":
        return { text: S.routes.on, class: "ok" };
      case "Paused":
        return { text: S.routes.paused, class: "warn" };
      case "Unavailable":
        return { text: S.routes.unavailable(state.reason), class: "bad" };
    }
  }

  const clock = (seconds: number): string => `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

  // A box shows what the router says, not the click: it moves once the answer or the next read does.
  async function toggle(event: Event & { currentTarget: HTMLInputElement }): Promise<void> {
    event.currentTarget.checked = on;
    const answer = await change(S.failed.router, () => commands.setRouterEnabled(!on));
    if (answer) status = answer;
  }

  async function setEnabled(event: Event & { currentTarget: HTMLInputElement }, name: string, enabled: boolean): Promise<void> {
    event.currentTarget.checked = !enabled;
    await change(S.failed.route, () => commands.setRouteEnabled(name, enabled));
    void refresh();
  }

  function showAll(value: boolean): void {
    all = value;
    void refresh();
  }

  async function addPreset(event: Event & { currentTarget: HTMLSelectElement }): Promise<void> {
    const preset = event.currentTarget.value as Preset;
    event.currentTarget.value = "";
    await change(S.failed.preset, () => commands.addRoutePreset(preset));
    void refresh();
  }

  async function remove(name: string): Promise<void> {
    const sure = await confirmation.ask({ title: S.routes.deleteTitle(name), text: S.routes.deleteText, yes: S.routes.delete, danger: true });
    if (!sure) return;
    await change(S.failed.deleteRoute, () => commands.deleteRoute(name));
    void refresh();
  }
</script>

{#if status}
  {@const state = stateLabel(status.state)}
  <section class="card">
    <div class="row">
      <label class="inline">
        <input type="checkbox" checked={on} disabled={app.busy} onchange={toggle} />
        <strong>{S.routes.title}</strong>
      </label>
      <span class={state.class}>{state.text}</span>
    </div>
    {#if on}
      <p class={status.proxy.state === "Ok" ? "ok" : status.proxy.state === "Unknown" ? "muted" : "bad"}>{proxyLabel(status.proxy)}</p>
    {/if}
    <p>{S.routes.proxied(trafficLabel(status.totals.proxied))}</p>
    <p>{S.routes.direct(trafficLabel(status.totals.direct))}</p>
    {#if error}<pre class="bad">{error}</pre>{/if}
    {#if dohBlind}<p class="warn">{S.routes.systemDoh}</p>{/if}
  </section>

  <section class="card">
    {#if editing}
      <RouteEditor
        rule={editing.rule}
        create={editing.create}
        onClose={() => {
          editing = null;
          void refresh();
        }}
      />
    {:else}
      <h2>{S.routes.rules}</h2>
      {#if view && view.rules.length > 0}
        <table>
          <tbody>
            {#each view.rules as row (row.rule.name)}
              <tr>
                <td>
                  <input
                    type="checkbox"
                    aria-label={S.routes.enabled}
                    checked={row.rule.enabled}
                    disabled={app.busy}
                    onchange={(event) => setEnabled(event, row.rule.name, !row.rule.enabled)}
                  />
                </td>
                <td><strong>{row.rule.name}</strong></td>
                <td class="muted">{routeSummary(row.rule)}</td>
                <td class="muted">{trafficLabel(row.traffic)}</td>
                <td class="actions">
                  <button onclick={() => (editing = { rule: $state.snapshot(row.rule), create: false })}>{S.routes.edit}</button>
                  <button class="danger" disabled={app.busy} onclick={() => remove(row.rule.name)}>{S.routes.delete}</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="muted">{S.routes.noRules}</p>
      {/if}
      <div class="buttons">
        <button
          onclick={() => (editing = { rule: { name: "", enabled: true, protocol: "udp", ports: [], ips: [], domains: [], apps: [] }, create: true })}
        >
          {S.routes.newRule}
        </button>
        <select aria-label={S.routes.addPreset} disabled={app.busy} onchange={addPreset}>
          <option value="" selected disabled>{S.routes.addPreset}</option>
          {#each Object.entries(S.routes.presets) as [preset, label] (preset)}
            <option value={preset}>{label}</option>
          {/each}
        </select>
      </div>
    {/if}
  </section>

  <section class="card">
    <div class="row">
      <h2 class="grow">{S.routes.flows}</h2>
      <label class="inline"><input type="radio" name="flows" checked={!all} onchange={() => showAll(false)} /> {S.routes.onlyRules}</label>
      <label class="inline"><input type="radio" name="flows" checked={all} onchange={() => showAll(true)} /> {S.routes.all}</label>
    </div>
    {#if flows.length > 0}
      <table>
        <thead>
          <tr>
            <th>{S.routes.columns.protocol}</th>
            <th>{S.routes.columns.destination}</th>
            <th>{S.routes.columns.domain}</th>
            <th>{S.routes.columns.program}</th>
            <th>{S.routes.columns.rule}</th>
            <th>{S.routes.columns.via}</th>
            <th>{S.routes.columns.up}</th>
            <th>{S.routes.columns.down}</th>
            <th>{S.routes.columns.time}</th>
          </tr>
        </thead>
        <tbody>
          <!-- A flow has no id of its own: rows go by position. -->
          {#each flows as flow, i (i)}
            <tr class:muted={flow.closed}>
              <td>{flow.protocol}</td>
              <td>{flow.destination}</td>
              <td>{flow.domain ?? "—"}</td>
              <td title={flow.program ?? undefined}>{programName(flow.program)}</td>
              <td>{flow.rule ?? "—"}</td>
              <td>{viaLabel(flow.via)}</td>
              <td>{bytes(flow.up ?? 0)}</td>
              <td>{bytes(flow.down ?? 0)}</td>
              <td>{clock(flow.seconds)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <p class="muted">{S.routes.noFlows}</p>
    {/if}
  </section>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<style>
  .actions {
    text-align: right;
  }
</style>

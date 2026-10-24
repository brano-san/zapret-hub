<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type Exclusions, type FlowRow, type Preset, type RouterState, type RouterStatus, type RoutesView, type Rule } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { notices } from "$lib/notices.svelte";
  import { bytes, lines, programName, proxyLabel, routeSummary, trafficLabel, viaLabel } from "$lib/format";
  import { S } from "$lib/strings";
  import RouteEditor from "./RouteEditor.svelte";

  let status = $state<RouterStatus | null>(null);
  let view = $state<RoutesView | null>(null);
  let flows = $state<FlowRow[]>([]);
  let all = $state(false);
  let editing = $state<{ rule: Rule; create: boolean } | null>(null);
  /** The capabilities are granted; they take effect on the hub's next start. */
  let granted = $state(false);

  /** The exclusions as read, and the fields as typed. */
  let exclusions = $state<Exclusions | null>(null);
  let excl = $state({ domains: "", ips: "", ports: "" });
  let exclSaved = $state(false);

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

  function takeExclusions(read: Exclusions): void {
    exclusions = read;
    excl = { domains: read.domains.join("\n"), ips: read.ips.join("\n"), ports: read.ports.join(", ") };
  }

  onMount(() => {
    void call(S.failed.exclusions, commands.getExclusions()).then((read) => read && takeExclusions(read));

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

  async function grant(): Promise<void> {
    if ((await change(S.failed.grant, () => commands.grantRouting())) !== undefined) granted = true;
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

  async function addPreset(preset: Preset): Promise<void> {
    await change(S.failed.preset, () => commands.addRoutePreset(preset));
    void refresh();
  }

  async function remove(name: string): Promise<void> {
    const sure = await confirmation.ask({ title: S.routes.deleteTitle(name), text: S.routes.deleteText, yes: S.routes.delete, danger: true });
    if (!sure) return;
    await change(S.failed.deleteRoute, () => commands.deleteRoute(name));
    void refresh();
  }

  /** The closed rows live here alone: the next read brings the open flows back as they are. */
  function clearFlows(): void {
    flows = flows.filter((flow) => !flow.closed);
  }

  async function saveExclusions(): Promise<void> {
    if (!exclusions) return;
    const next: Exclusions = {
      domains: lines(excl.domains),
      ips: lines(excl.ips),
      // The ports skip the engine on Linux only: Windows keeps what it has.
      ports: app.linux ? excl.ports.split(/[\s,]+/).filter(Boolean) : exclusions.ports,
    };
    if ((await change(S.failed.exclusions, () => commands.saveExclusions(next))) === undefined) return;
    takeExclusions(next);
    exclSaved = true;
    setTimeout(() => (exclSaved = false), 4000);
  }
</script>

{#if status}
  {@const state = stateLabel(status.state)}
  {#if status.needs_grant}
    <section class="card">
      <h2>{S.routes.grant.title}</h2>
      <p class="muted">{S.routes.grant.text}</p>
      {#if granted}
        <p class="ok">{S.routes.grant.done}</p>
      {:else}
        <div class="buttons">
          <button class="primary" disabled={app.busy} onclick={grant}>{S.routes.grant.button}</button>
        </div>
      {/if}
    </section>
  {/if}
  <section class="card">
    <div class="row">
      <label class="inline">
        <input type="checkbox" checked={on} disabled={app.busy} onchange={toggle} />
        <strong>{S.routes.title}</strong>
      </label>
      <span class={state.class}>{state.text}</span>
    </div>
    <p class="muted">{S.routes.about}</p>
    {#if app.settings?.socks5}
      <p>
        {S.routes.socks5(app.settings.socks5)}
        {#if on}
          <span class={status.proxy.state === "Ok" ? "ok" : status.proxy.state === "Unknown" ? "muted" : "bad"}>{proxyLabel(status.proxy)}</span>
        {/if}
      </p>
    {:else}
      <p class="warn">
        {S.routes.proxyMissing}
        <button class="link" onclick={() => app.setTab("settings")}>{S.routes.proxyGo}</button>
      </p>
    {/if}
    <p>{S.routes.proxied(trafficLabel(status.totals.proxied))} · {S.routes.direct(trafficLabel(status.totals.direct))}</p>
    {#if error}<pre class="bad">{error}</pre>{/if}
    {#if dohBlind}<p class="warn">{app.linux ? S.routes.systemDohLinux : S.routes.systemDoh}</p>{/if}

    <hr />

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
      </div>
      <div class="row chips">
        <span class="muted">{S.routes.addPreset}:</span>
        {#each Object.entries(S.routes.presets) as [preset, label] (preset)}
          <button disabled={app.busy} onclick={() => addPreset(preset as Preset)}>{label}</button>
        {/each}
      </div>
    {/if}
  </section>

  <section class="card">
    <div class="row">
      <h2 class="grow">{S.routes.flows}</h2>
      <button disabled={!flows.some((flow) => flow.closed)} onclick={clearFlows}>{S.routes.clearFlows}</button>
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
              <td>
                <span class="badge" class:ok={flow.via.via === "Proxy" || flow.via.via === "Opening"} class:muted={flow.via.via !== "Proxy" && flow.via.via !== "Opening"}
                  >{viaLabel(flow.via)}</span
                >
              </td>
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

<section class="card">
  <h2>{S.routes.exclusions.title}</h2>
  <p class="muted">{S.routes.exclusions.about}</p>
  <div class="cols">
    <label>
      <span>{S.routes.exclusions.domains} <span class="badge">{exclusions?.domains.length ?? "…"}</span></span>
      <span class="muted hint">{S.routes.exclusions.domainsHint}</span>
      <textarea rows="6" spellcheck="false" bind:value={excl.domains} disabled={!exclusions}></textarea>
    </label>
    <label>
      <span>{S.routes.exclusions.ips} <span class="badge">{exclusions?.ips.length ?? "…"}</span></span>
      <span class="muted hint">{S.routes.exclusions.ipsHint}</span>
      <textarea rows="6" spellcheck="false" bind:value={excl.ips} disabled={!exclusions}></textarea>
    </label>
  </div>
  {#if app.linux}
    <label class="ports">
      <span>{S.routes.exclusions.ports}</span>
      <input bind:value={excl.ports} placeholder={S.routes.exclusions.portsPlaceholder} disabled={!exclusions} />
    </label>
    <p class="muted hint">{S.routes.exclusions.bypassHint}</p>
  {/if}
  <div class="buttons">
    <button class="primary" disabled={!exclusions || app.busy} onclick={saveExclusions}>{S.common.save}</button>
    {#if exclSaved}<span class="ok">{S.settings.saved}</span>{/if}
  </div>
</section>

<style>
  section.card {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  section.card p,
  section.card h2 {
    margin: 0;
  }

  hr {
    width: 100%;
    margin: 4px 0;
    border: none;
    border-top: 1px solid var(--border);
  }

  .actions {
    text-align: right;
    white-space: nowrap;
  }

  .chips {
    flex-wrap: wrap;
  }

  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 16px;
  }

  .cols label,
  .ports {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ports input {
    max-width: 360px;
  }
</style>

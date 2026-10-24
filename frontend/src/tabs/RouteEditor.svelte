<script lang="ts">
  import { commands, type FieldError, type Protocol, type Rule } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import Select from "../components/Select.svelte";
  import { lines, routeDraft, routeFieldText } from "$lib/format";
  import { notices } from "$lib/notices.svelte";
  import { S } from "$lib/strings";

  interface Props {
    /** The rule the editor starts with: an empty one for a new rule. */
    rule: Rule;
    /** A new rule: its name is typed here; a saved one keeps its name (no renaming). */
    create: boolean;
    onClose: () => void;
  }

  let { rule, create, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(rule.name);
  // svelte-ignore state_referenced_locally
  let enabled = $state(rule.enabled);
  // svelte-ignore state_referenced_locally
  let protocol = $state<Protocol>(rule.protocol);
  // svelte-ignore state_referenced_locally
  let ports = $state(rule.ports.join(", "));
  // svelte-ignore state_referenced_locally
  let ips = $state(rule.ips.join("\n"));
  // svelte-ignore state_referenced_locally
  let domains = $state(rule.domains.join("\n"));
  // svelte-ignore state_referenced_locally
  let apps = $state(rule.apps.join("\n"));
  let errors = $state<FieldError[]>([]);
  let picker = $state<HTMLInputElement>();

  // A fresh editor is clean: red errors appear only after the field was touched or «Сохранить» was pressed.
  const dirty = new Set<string>();
  let touched = $state(0);
  let attempted = $state(false);

  const mark = (field: string): void => {
    // First of a Set: `$state` wraps it without proxies.
    dirty.add(field);
    touched++;
  };
  const shown = (field: string): string[] => {
    // The Set itself is not reactive: touching `touched` is what makes the read again.
    void touched;
    return attempted || dirty.has(field)
      ? errors.filter((error) => error.field === field).map((error) => routeFieldText(error))
      : [];
  };

  const draft = $derived(routeDraft({ name, enabled, protocol, ports, ips, domains, apps }));

  // Checked on every change; an answer for an older draft is dropped.
  // Plain variable: one IPC failure gives one notice, not one per keystroke.
  let validateFailing = false;
  $effect(() => {
    const asked = draft;
    commands
      .validateRoute(asked)
      .then((list) => {
        validateFailing = false;
        if (asked === draft) errors = list;
      })
      .catch((err: unknown) => {
        if (asked !== draft || validateFailing) return;
        validateFailing = true;
        notices.push({ level: "Error", title: S.failed.validate, text: String(err) });
      });
  });

  const protocolOptions = Object.entries(S.routes.protocols).map(([value, label]) => ({ value: value as Protocol, label }));

  async function importFile(event: Event & { currentTarget: HTMLInputElement }): Promise<void> {
    const input = event.currentTarget;
    const file = input.files?.[0];
    // The same file can be picked again.
    input.value = "";
    if (!file) return;
    const merged = await call(S.failed.importRoute, commands.importRouteLines(lines(ips), lines(domains), await file.text()));
    if (merged) {
      ips = merged.ips.join("\n");
      domains = merged.domains.join("\n");
    }
  }

  async function save(): Promise<void> {
    // The check may lag a keystroke: if it saw a problem, show it instead of a failed save.
    if (errors.length > 0) {
      attempted = true;
      touched++;
      return;
    }
    const saving = draft;
    if ((await change(S.failed.saveRoute, () => commands.saveRoute(saving, create))) !== undefined) onClose();
  }
</script>

{#snippet fieldErrors(field: string)}
  {#each shown(field) as message (message)}<p class="bad hint">{message}</p>{/each}
{/snippet}

<div class="top">
  <label for="route-name">{S.routes.name}</label>
  <input id="route-name" bind:value={name} readonly={!create} oninput={() => mark("name")} />
  <label class="inline"><input type="checkbox" bind:checked={enabled} /> {S.routes.enabled}</label>
  <span class="muted">{S.routes.protocol}</span>
  <Select bind:value={protocol} options={protocolOptions} ariaLabel={S.routes.protocol} />
  <label for="route-ports">{S.routes.ports}</label>
  <input id="route-ports" class="ports" bind:value={ports} placeholder={S.routes.portPlaceholder} oninput={() => mark("ports")} />
</div>
{@render fieldErrors("name")}
{@render fieldErrors("ports")}

<div class="cols">
  <div>
    <div class="field-header">
      <label for="route-domains">{S.routes.cols.domains}</label>
      <span class="muted hint">{S.routes.colHints.domains}</span>
    </div>
    <textarea id="route-domains" rows="6" bind:value={domains} oninput={() => mark("domains")}></textarea>
    {@render fieldErrors("domains")}
  </div>
  <div>
    <div class="field-header">
      <label for="route-ips">{S.routes.cols.ips}</label>
      <span class="muted hint">{S.routes.colHints.ips}</span>
    </div>
    <textarea id="route-ips" rows="6" bind:value={ips} oninput={() => mark("ips")}></textarea>
    {@render fieldErrors("ips")}
  </div>
  <div>
    <div class="field-header">
      <label for="route-apps">{S.routes.cols.apps}</label>
      <span class="muted hint">{S.routes.colHints.apps}</span>
    </div>
    <textarea id="route-apps" rows="6" bind:value={apps} oninput={() => mark("apps")}></textarea>
    {@render fieldErrors("apps")}
  </div>
</div>
{#if draft.apps.length > 0 && draft.ports.length === 0 && draft.protocol !== "tcp"}<p class="muted hint">{S.routes.appsHint}</p>{/if}
{#if draft.domains.length > 0}<p class="muted hint">{S.routes.dohHint}</p>{/if}
<input bind:this={picker} type="file" accept=".txt" hidden onchange={importFile} />
<div class="buttons">
  <button onclick={() => picker?.click()}>{S.routes.import}</button>
  <span class="grow"></span>
  <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
  <button onclick={onClose}>{S.common.close}</button>
</div>
<p class="muted hint">{S.routes.liveHint}</p>

<style>
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .top input[readonly] {
    min-width: 200px;
  }
  .top .ports {
    width: 240px;
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 16px;
    margin: 12px 0;
    /* Equal columns: headers and textareas start on the same line in each of them. */
    align-items: start;
  }
  .cols > div {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cols textarea {
    flex: 1;
  }
  .field-header {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field-header .hint {
    margin: 0;
  }
  .buttons {
    margin: 12px 0;
  }
</style>

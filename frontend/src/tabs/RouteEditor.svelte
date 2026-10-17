<script lang="ts">
  import { commands, type FieldError, type Protocol, type Rule } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { lines, routeDraft } from "$lib/format";
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
  let ports = $state(rule.ports.join("\n"));
  // svelte-ignore state_referenced_locally
  let ips = $state(rule.ips.join("\n"));
  // svelte-ignore state_referenced_locally
  let domains = $state(rule.domains.join("\n"));
  // svelte-ignore state_referenced_locally
  let apps = $state(rule.apps.join("\n"));
  let errors = $state<FieldError[]>([]);
  let picker = $state<HTMLInputElement>();

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

  const errorsOf = (field: string): string[] => errors.filter((error) => error.field === field).map((error) => error.message);

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
    const saving = draft;
    if ((await change(S.failed.saveRoute, () => commands.saveRoute(saving, create))) !== undefined) onClose();
  }
</script>

{#snippet fieldErrors(field: string)}
  {#each errorsOf(field) as message (message)}<p class="bad hint">{message}</p>{/each}
{/snippet}

<label for="route-name">{S.routes.name}</label>
<input id="route-name" class="wide" bind:value={name} readonly={!create} />
{@render fieldErrors("name")}
<label class="inline"><input type="checkbox" bind:checked={enabled} /> {S.routes.enabled}</label>
<label for="route-protocol">{S.routes.protocol}</label>
<select id="route-protocol" bind:value={protocol}>
  {#each Object.entries(S.routes.protocols) as [value, label] (value)}
    <option {value}>{label}</option>
  {/each}
</select>
{@render fieldErrors("protocol")}
<label for="route-ports">{S.routes.ports}</label>
<textarea id="route-ports" rows="3" bind:value={ports}></textarea>
{@render fieldErrors("ports")}
<label for="route-ips">{S.routes.ips}</label>
<textarea id="route-ips" rows="6" bind:value={ips}></textarea>
{@render fieldErrors("ips")}
<label for="route-domains">{S.routes.domains}</label>
<textarea id="route-domains" rows="6" bind:value={domains}></textarea>
{@render fieldErrors("domains")}
<label for="route-apps">{S.routes.apps}</label>
<textarea id="route-apps" rows="3" bind:value={apps}></textarea>
{@render fieldErrors("apps")}
{#if draft.apps.length > 0 && draft.ports.length === 0 && draft.protocol !== "tcp"}<p class="muted hint">{S.routes.appsHint}</p>{/if}
{#if draft.domains.length > 0}<p class="muted hint">{S.routes.dohHint}</p>{/if}
<input bind:this={picker} type="file" accept=".txt" hidden onchange={importFile} />
<div class="buttons">
  <button onclick={() => picker?.click()}>{S.routes.import}</button>
  <span class="grow"></span>
  <button class="primary" disabled={app.busy || errors.length > 0} onclick={save}>{S.common.save}</button>
  <button onclick={onClose}>{S.common.close}</button>
</div>

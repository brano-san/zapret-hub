<script lang="ts">
  import { commands, type FieldError, type Rule } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
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
  let ports = $state(rule.ports.join("\n"));
  // svelte-ignore state_referenced_locally
  let ips = $state(rule.ips.join("\n"));
  let errors = $state<FieldError[]>([]);
  let picker = $state<HTMLInputElement>();

  const lines = (text: string): string[] =>
    text
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);

  const draft = $derived<Rule>({ name: name.trim(), enabled, protocol: rule.protocol, ports: lines(ports), ips: lines(ips) });

  // Checked on every change; an answer for an older draft is dropped.
  $effect(() => {
    const asked = draft;
    void commands.validateRoute(asked).then((list) => {
      if (asked === draft) errors = list;
    });
  });

  const errorsOf = (field: string): string[] => errors.filter((error) => error.field === field).map((error) => error.message);

  async function importFile(event: Event & { currentTarget: HTMLInputElement }): Promise<void> {
    const input = event.currentTarget;
    const file = input.files?.[0];
    // The same file can be picked again.
    input.value = "";
    if (!file) return;
    const merged = await call(S.failed.importRoute, commands.importRouteLines(lines(ips), await file.text()));
    if (merged) ips = merged.join("\n");
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
<p>{S.routes.protocol}</p>
{@render fieldErrors("protocol")}
<label for="route-ports">{S.routes.ports}</label>
<textarea id="route-ports" rows="3" bind:value={ports}></textarea>
{@render fieldErrors("ports")}
<label for="route-ips">{S.routes.ips}</label>
<textarea id="route-ips" rows="6" bind:value={ips}></textarea>
{@render fieldErrors("ips")}
<input bind:this={picker} type="file" accept=".txt" hidden onchange={importFile} />
<div class="buttons">
  <button onclick={() => picker?.click()}>{S.routes.import}</button>
  <span class="grow"></span>
  <button class="primary" disabled={app.busy || errors.length > 0} onclick={save}>{S.common.save}</button>
  <button onclick={onClose}>{S.common.close}</button>
</div>

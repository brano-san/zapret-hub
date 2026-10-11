<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { ZAPRET2, nameError, quickSummary } from "$lib/format";
  import { S } from "$lib/strings";
  import CircularBuilder from "../components/CircularBuilder.svelte";

  interface Props {
    /** Without the prefix: `mine` for `zapret2:mine`. */
    name: string;
    /** The strategy whose text the editor starts with: this one, or the one a copy is made of. */
    from: string;
    /** A text to start with instead of `from`'s: a new strategy from scratch. */
    initial?: string;
    /** A copy: saved as a new strategy. */
    create: boolean;
    /** The full name of what was saved, `null` when nothing was. */
    onClose: (saved: string | null) => void;
  }

  let { name, from, initial, create, onClose }: Props = $props();

  // A copy or a new one is named here until it is saved; a saved one keeps its name.
  // svelte-ignore state_referenced_locally
  let current = $state(name);
  const currentError = $derived(create ? nameError(current) : null);
  let text = $state<string | null>(null);
  let status = $state("");
  let loaded = $state(false);
  /** The circular builder is open over the text. */
  let circular = $state(false);

  onMount(async () => {
    if (initial !== undefined) {
      text = initial;
      loaded = true;
      return;
    }
    text = (await call(S.failed.strategy, commands.getStrategy(from)))?.source ?? null;
    loaded = true;
  });

  async function validate(): Promise<void> {
    if (text === null) return;
    const draft = text;
    status = "";
    if ((await change(S.failed.validate, () => commands.validateZapret2(draft))) !== undefined) status = S.zapret2.valid;
  }

  async function save(): Promise<void> {
    if (text === null) return;
    const draft = text;
    status = "";
    const saved = current.trim();
    const check = await change(S.failed.saveCustom, () => commands.saveZapret2(saved, draft, create));
    if (check === undefined) return;
    // The active strategy was installed again: its quick check is worth reading before closing.
    if (check) status = `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}`;
    else onClose(ZAPRET2 + saved);
  }
</script>

{#if text !== null}
  <h2>{ZAPRET2 + current.trim()}</h2>
  {#if create}
    <div class="row">
      <label for="zapret2-name">{S.strategies.newName}</label>
      <input id="zapret2-name" class="grow" bind:value={current} />
    </div>
    {#if currentError}<p class="bad">{currentError}</p>{/if}
  {/if}
  <div class="row">
    <p class="muted grow">{S.zapret2.hint}</p>
    <button onclick={() => (circular = !circular)}>{circular ? S.circular.hide : S.circular.open}</button>
  </div>
  {#if circular}
    <CircularBuilder bind:text={() => text ?? "", (value) => (text = value)} />
  {/if}
  <textarea rows="24" spellcheck="false" bind:value={text} aria-label={S.zapret2.text}></textarea>
  <div class="buttons">
    <span class="grow"></span>
    <button disabled={app.busy} onclick={validate}>{S.editor.validate}</button>
    <button class="primary" disabled={app.busy || currentError !== null} onclick={save}>{S.common.save}</button>
    <button onclick={() => onClose(null)}>{S.common.close}</button>
  </div>
  {#if status}<pre>{status}</pre>{/if}
{:else}
  <p class="muted">{loaded ? S.failed.strategy : S.common.loading}</p>
  {#if loaded}<div class="buttons"><button onclick={() => onClose(null)}>{S.common.close}</button></div>{/if}
{/if}

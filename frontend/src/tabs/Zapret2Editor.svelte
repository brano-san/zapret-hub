<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { ZAPRET2, quickSummary } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** Without the prefix: `mine` for `zapret2:mine`. */
    name: string;
    /** The strategy whose text the editor starts with: this one, or the one a copy is made of. */
    from: string;
    /** A copy: saved as a new strategy. */
    create: boolean;
    /** The full name of what was saved, `null` when nothing was. */
    onClose: (saved: string | null) => void;
  }

  let { name, from, create, onClose }: Props = $props();

  let text = $state<string | null>(null);
  let status = $state("");

  onMount(async () => {
    text = (await call(S.failed.strategy, commands.getStrategy(from)))?.source ?? null;
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
    const check = await change(S.failed.saveCustom, () => commands.saveZapret2(name, draft, create));
    if (check === undefined) return;
    // The active strategy was installed again: its quick check is worth reading before closing.
    if (check) status = `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}`;
    else onClose(ZAPRET2 + name);
  }
</script>

{#if text !== null}
  <h2>{ZAPRET2 + name}</h2>
  <p class="muted">{S.zapret2.hint}</p>
  <textarea rows="24" spellcheck="false" bind:value={text} aria-label={S.zapret2.text}></textarea>
  <div class="buttons">
    <span class="grow"></span>
    <button disabled={app.busy} onclick={validate}>{S.editor.validate}</button>
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    <button onclick={() => onClose(null)}>{S.common.close}</button>
  </div>
  {#if status}<pre>{status}</pre>{/if}
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type CustomStrategy } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { blockSummary, quickSummary } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** The file stem, `general (HUB <name>)`. */
    stem: string;
    onClose: () => void;
  }

  let { stem, onClose }: Props = $props();

  let draft = $state<CustomStrategy | null>(null);
  let status = $state("");

  onMount(async () => {
    draft = (await call(S.failed.strategy, commands.getStrategy(stem)))?.record ?? null;
  });

  function move(index: number, by: number): void {
    if (!draft) return;
    const to = index + by;
    if (to < 0 || to >= draft.blocks.length) return;
    const [block] = draft.blocks.splice(index, 1);
    draft.blocks.splice(to, 0, block ?? "");
  }

  function duplicate(index: number): void {
    if (draft) draft.blocks.splice(index + 1, 0, draft.blocks[index] ?? "");
  }

  function removeBlock(index: number): void {
    if (draft) draft.blocks.splice(index, 1);
  }

  async function validate(): Promise<void> {
    if (!draft) return;
    const strategy = draft;
    status = "";
    if ((await change(S.failed.validate, () => commands.validateCustom(strategy))) !== undefined) status = S.editor.valid;
  }

  async function save(): Promise<void> {
    if (!draft) return;
    const strategy = draft;
    status = "";
    const check = await change(S.failed.saveCustom, () => commands.saveCustom(strategy, false));
    if (check === undefined) return;
    // The active strategy was installed again: its quick check is worth reading before closing.
    if (check) status = `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}`;
    else onClose();
  }
</script>

{#if draft}
  <h2>{stem}</h2>
  <p class="muted">{S.editor.hint}</p>
  <label for="wf">{S.editor.filter}</label>
  <input id="wf" class="wide" bind:value={draft.wf} />
  {#each draft.blocks as block, i (i)}
    <div class="block">
      <div class="row">
        <strong>{S.strategies.block(i + 1)}</strong>
        <span class="muted">{blockSummary(block)}</span>
        <span class="grow"></span>
        <button title={S.editor.up} aria-label={S.editor.up} disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
        <button
          title={S.editor.down}
          aria-label={S.editor.down}
          disabled={i === draft.blocks.length - 1}
          onclick={() => move(i, 1)}>↓</button
        >
        <button onclick={() => duplicate(i)}>{S.editor.duplicate}</button>
        <button class="danger" disabled={draft.blocks.length === 1} onclick={() => removeBlock(i)}>{S.editor.remove}</button>
      </div>
      <textarea rows="3" bind:value={draft.blocks[i]} aria-label={S.strategies.block(i + 1)}></textarea>
    </div>
  {/each}
  <div class="buttons">
    <button onclick={() => draft?.blocks.push("")}>{S.editor.add}</button>
    <span class="grow"></span>
    <button disabled={app.busy} onclick={validate}>{S.editor.validate}</button>
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    <button onclick={onClose}>{S.common.close}</button>
  </div>
  {#if status}<pre>{status}</pre>{/if}
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<style>
  .block {
    margin: 10px 0;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  /* A long summary wraps, the block number does not («Блок / 3»). */
  strong {
    white-space: nowrap;
  }
</style>

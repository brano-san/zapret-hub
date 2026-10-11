<script lang="ts">
  import { commands, type UserList } from "$lib/ipc/bindings";
  import { change } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  interface Props {
    list: UserList;
    entries: string[];
    disabled: boolean;
  }

  let { list, entries, disabled }: Props = $props();
  let entry = $state("");

  const text = $derived(S.zapret.lists[list]);

  async function add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const value = entry.trim();
    if (value && (await change(S.failed.list, () => commands.addListEntry(list, value))) !== undefined) entry = "";
  }
</script>

<div>
  <h3>{text.title} <span class="muted">{text.file}</span></h3>
  <p class="muted">{text.hint}</p>
  <form class="row" onsubmit={add}>
    <input class="grow" placeholder={text.placeholder} bind:value={entry} {disabled} aria-label={text.title} />
    <button type="submit" disabled={disabled || !entry.trim()}>{S.zapret.add}</button>
  </form>
  <ul>
    {#each entries as value}
      <li>
        <code>{value}</code>
        <button class="link" {disabled} onclick={() => change(S.failed.list, () => commands.removeListEntry(list, value))}>
          {S.zapret.removeEntry}
        </button>
      </li>
    {/each}
  </ul>
</div>

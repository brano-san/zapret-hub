<script lang="ts">
  import { onDestroy } from "svelte";
  import { commands, events, type Engine } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { localizeError } from "$lib/format";
  import { notices } from "$lib/notices.svelte";
  import { S } from "$lib/strings";

  /** `ondone` gets the new folder: the caller puts it into its field and checks it. */
  let { engine, ondone }: { engine: Engine; ondone: (dir: string) => void | Promise<void> } = $props();

  let open = $state(false);
  let parent = $state("");
  let busy = $state(false);
  let canceled = false;
  let received = $state(0);
  let total = $state(0);
  let done = $state("");

  const unlisten = events.downloadProgress.listen(({ payload }) => {
    if (payload.engine !== engine) return;
    received = payload.received;
    total = payload.total;
  });
  onDestroy(() => void unlisten.then((stop) => stop()));

  const mb = (bytes: number) => (bytes / 1048576).toFixed(1);

  async function show(): Promise<void> {
    open = true;
    if (!parent) parent = (await commands.defaultDownloadParent()) ?? "";
  }

  async function pick(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path) parent = path;
  }

  async function start(): Promise<void> {
    busy = true;
    canceled = false;
    received = total = 0;
    done = "";
    const answer = await commands.downloadEngine(engine, parent.trim());
    busy = false;
    if (answer.status === "error") {
      if (!canceled) notices.push({ level: "Error", title: S.failed.download, text: localizeError(answer.error) });
      return;
    }
    done = S.download.done(answer.data.tag, answer.data.sha256);
    open = false;
    await ondone(answer.data.dir);
  }

  async function cancel(): Promise<void> {
    canceled = true;
    await commands.cancelDownload();
  }
</script>

{#if busy}
  <div class="row">
    <span class="muted grow">{received > 0 ? S.download.progress(mb(received), total > 0 ? mb(total) : null) : S.download.fetching}</span>
    <button onclick={cancel}>{S.common.cancel}</button>
  </div>
{:else if open}
  <p class="muted">{S.download.hint}</p>
  <div class="row">
    <input class="grow" bind:value={parent} aria-label={S.download.parent} placeholder={S.download.parent} />
    <button onclick={pick}>{S.settings.pick}</button>
    <button class="primary" disabled={!parent.trim()} onclick={start}>{S.download.start}</button>
    <button onclick={() => (open = false)}>{S.common.cancel}</button>
  </div>
{:else}
  <button class="link" onclick={show}>{S.download.button}</button>
{/if}
{#if done}<p class="muted sha">{done}</p>{/if}

<style>
  .sha {
    overflow-wrap: anywhere;
  }
</style>

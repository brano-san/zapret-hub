<script lang="ts">
  import { commands, type TgWsStatus } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { notices } from "$lib/notices.svelte";
  import { LINKS, S } from "$lib/strings";
  import CopyButton from "./CopyButton.svelte";

  /** `full`: the Tools tab's card with the port, secret, flags and log; else the Overview's short one. */
  let { full = false }: { full?: boolean } = $props();

  let tgws = $state<TgWsStatus | null>(null);
  let busy = $state(false);
  let form = $state({ port: "", secret: "", args: "" });
  let touched = $state(false);

  function take(next: TgWsStatus | undefined): void {
    if (!next) return;
    tgws = next;
    // The user's edits win: the status fills untouched fields only.
    if (!touched) form = { port: String(next.port), secret: next.secret, args: next.args };
  }

  $effect(() => {
    void app.revision;
    void call(S.failed.tgWsStatus, commands.tgWsStatus()).then(take);
  });

  async function run(title: string, action: () => ReturnType<typeof commands.tgWsStatus>): Promise<void> {
    busy = true;
    try {
      take(await change(title, action));
    } finally {
      busy = false;
    }
  }

  const install = () => run(S.failed.tgWsInstall, () => commands.installTgWs());
  const toggle = () => run(S.failed.tgWsEnabled, () => commands.setTgWsEnabled(!tgws?.running));

  async function save(): Promise<void> {
    const port = Number(form.port.trim());
    const bad = (text: string) => notices.push({ level: "Error", title: S.failed.tgWsConfig, text });
    if (!Number.isInteger(port) || port < 1 || port > 65535) return bad(S.tgws.badPort);
    if (!/^[0-9a-fA-F]{32}$/.test(form.secret.trim())) return bad(S.tgws.badSecret);
    await run(S.failed.tgWsConfig, () => commands.saveTgWsConfig(port, form.secret.trim(), form.args.trim()));
    touched = false;
  }

  function generate(): void {
    form.secret = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
    touched = true;
  }

  const openRepo = () => call(S.failed.open, commands.openUrl(LINKS.tgWsProxy));
</script>

<section class="card tgws">
  <div class="head">
    <h2>{S.tgws.title}</h2>
    <span class="badge" class:ok={tgws?.running} class:muted={!tgws?.running}>
      {#if !tgws?.installed}{S.tgws.absent}{:else if tgws.running}{S.tgws.on(tgws.port)}{:else}{S.tgws.off}{/if}
    </span>
  </div>
  <p class="muted">{S.tgws.about}</p>
  {#if tgws?.unavailable}<p class="warn">{S.tgws.unavailable(tgws.unavailable)}</p>{/if}

  {#if !tgws?.installed}
    <div class="buttons">
      <button class="primary" disabled={busy || !!tgws?.unavailable} onclick={install}>
        {busy ? S.common.loading : S.tgws.install}
      </button>
      {#if full}<button class="link" onclick={openRepo}>{S.tgws.repo}</button>{/if}
    </div>
  {:else}
    {#if full}
      <p class="muted">{S.tgws.version(tgws.version)}</p>
      <div class="grid">
        <label class="field">
          <span class="muted">{S.tgws.port}</span>
          <input type="number" min="1" max="65535" bind:value={form.port} oninput={() => (touched = true)} />
        </label>
        <div class="field">
          <span class="muted">{S.tgws.secret}</span>
          <div class="row">
            <input bind:value={form.secret} aria-label={S.tgws.secret} oninput={() => (touched = true)} />
            <button title={S.tgws.generate} aria-label={S.tgws.generate} disabled={busy} onclick={generate}>↻</button>
          </div>
        </div>
        <label class="field wide">
          <span class="muted">{S.tgws.args}</span>
          <input bind:value={form.args} placeholder={S.tgws.argsPlaceholder} oninput={() => (touched = true)} />
        </label>
      </div>
    {/if}
    <div class="buttons">
      <button class="primary" disabled={busy} onclick={toggle}>{tgws.running ? S.tgws.disable : S.tgws.enable}</button>
      {#if full}
        <button disabled={busy || !touched} onclick={save}>{S.common.save}</button>
        <button disabled={busy || !!tgws.unavailable} onclick={install}>{busy ? S.common.loading : S.tgws.update}</button>
      {/if}
      <button disabled={busy} onclick={() => change(S.failed.tgWsOpen, () => commands.openTgProxy())}>{S.tgws.connect}</button>
      <CopyButton text={() => tgws?.link ?? ""} />
      {#if full}<button class="link" onclick={openRepo}>{S.tgws.repo}</button>{/if}
    </div>
    {#if full && tgws.logs}
      <details>
        <summary>{S.tgws.logs}</summary>
        <pre>{tgws.logs}</pre>
      </details>
    {/if}
  {/if}
</section>

<style>
  .tgws {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .tgws h2,
  .tgws p,
  .tgws pre,
  .tgws .buttons {
    margin: 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  /* Port 88px, the secret and the flags take the rest of the width. */
  .grid {
    display: grid;
    grid-template-columns: 88px 1fr;
    gap: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .wide {
    grid-column: 1 / -1;
  }

  .field input,
  .row input {
    min-width: 0;
    width: 100%;
  }

  .row {
    display: flex;
    gap: 8px;
  }

  pre {
    max-height: 200px;
    overflow: auto;
  }
</style>

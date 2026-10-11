<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { commands, type FolderCheck, type Info, type ProxyStatus, type Settings, type Theme } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { follow } from "$lib/draft";
  import { ms } from "$lib/format";
  import { LINKS, S } from "$lib/strings";
  import DownloadEngine from "../components/DownloadEngine.svelte";

  const THEMES: Theme[] = ["system", "light", "dark"];

  // A copy to edit, saved as a whole, and the settings it was last copied from.
  let draft = $state<Settings | null>(null);
  let from: Settings | null = null;
  let folder = $state<FolderCheck | null>(null);
  let zapret2 = $state<Info | null>(null);
  let proxy = $state<ProxyStatus | null>(null);
  let saved = $state(false);
  let version = $state("");

  async function checkFolder(): Promise<void> {
    const path = draft?.zapret_dir?.trim();
    folder = path ? ((await call(S.failed.folder, commands.checkFolder(path))) ?? null) : null;
  }

  async function pick(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path && draft) {
      draft.zapret_dir = path;
      await checkFolder();
    }
  }

  async function pickZapret2(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path && draft) draft.zapret2_dir = path;
  }

  /** The saved folder's version, or why it cannot be used: a new folder is checked by «Сохранить». */
  async function checkZapret2(): Promise<void> {
    zapret2 = (await call(S.failed.zapret2, commands.zapret2Info())) ?? null;
  }

  async function checkProxy(): Promise<void> {
    proxy = null;
    const addr = draft?.socks5?.trim();
    if (addr) proxy = (await call(S.failed.proxy, commands.checkProxy(addr))) ?? null;
  }

  async function save(): Promise<void> {
    if (!draft) return;
    const settings: Settings = {
      ...draft,
      zapret_dir: draft.zapret_dir?.trim() || null,
      zapret2_dir: draft.zapret2_dir?.trim() || null,
      socks5: draft.socks5?.trim() || null,
    };
    // The draft as sent: anything typed while the save runs stays an edit.
    const sent = $state.snapshot(draft);
    saved = false;
    if ((await change(S.failed.settings, () => commands.saveSettings(settings))) !== undefined) {
      // Saved: the draft as sent counts as unedited, so the settings read back replace it.
      from = sent;
      await loadSettings();
      saved = true;
    }
  }

  // Each read of the settings: the first, after a save, after every StateChanged.
  $effect(() => {
    const next = app.settings;
    if (!next) return;
    untrack(() => {
      const copy = follow(draft, from, next);
      if (copy === draft) return;
      // A «Проверить» result belongs to the address it checked.
      if (copy.socks5 !== draft?.socks5) proxy = null;
      draft = copy;
      from = next;
      void checkFolder();
      void checkZapret2();
    });
  });

  onMount(() => void commands.appVersion().then((v) => (version = v)));
</script>

{#if draft}
  <section class="card">
    <h2>{S.settings.folder}</h2>
    <p class="muted">{S.settings.folderOptional}</p>
    <div class="row">
      <input class="grow" bind:value={draft.zapret_dir} onchange={checkFolder} placeholder={S.settings.folderHint} />
      <button onclick={pick}>{S.settings.pick}</button>
    </div>
    {#if folder}
      <p class={folder.missing.length > 0 ? "bad" : "ok"}>
        {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
      </p>
    {/if}
    <DownloadEngine
      engine="Flowseal"
      ondone={async (dir) => {
        if (!draft) return;
        draft.zapret_dir = dir;
        await checkFolder();
      }}
    />
  </section>

  <section class="card">
    <h2>{S.settings.zapret2}</h2>
    <p class="muted">{S.settings.zapret2Hint}</p>
    <div class="row">
      <input
        class="grow"
        bind:value={draft.zapret2_dir}
        placeholder={S.settings.zapret2Example}
        aria-label={S.settings.zapret2}
      />
      <button onclick={pickZapret2}>{S.settings.pick}</button>
    </div>
    {#if zapret2?.dir}
      {#if zapret2.error}
        <pre class="bad">{zapret2.error}</pre>
      {:else}
        <p class="ok">{S.settings.zapret2Ok(zapret2.version, zapret2.compat)}</p>
      {/if}
    {/if}
    <DownloadEngine
      engine="Zapret2"
      ondone={async (dir) => {
        if (!draft) return;
        draft.zapret2_dir = dir;
        zapret2 = (await call(S.failed.zapret2, commands.checkZapret2Folder(dir))) ?? null;
      }}
    />
    <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.zapret2))}>
      {S.settings.zapret2Releases}
    </button>
  </section>

  <section class="card">
    <h2>{S.settings.proxy}</h2>
    <p class="muted">{S.settings.proxyHint}</p>
    <div class="row">
      <input class="grow" bind:value={draft.socks5} placeholder="127.0.0.1:2080" aria-label={S.settings.proxyAddress} />
      <button onclick={checkProxy}>{S.common.check}</button>
    </div>
    {#if proxy}
      <p class={proxy.ok ? "ok" : "bad"}>{proxy.ok ? S.overview.proxyOk(ms(proxy.latency_ms)) : S.overview.proxyFailed}</p>
      {#if proxy.error}<pre>{proxy.error}</pre>{/if}
    {/if}
  </section>

  <section class="card">
    <h2>{S.settings.start}</h2>
    <label class="inline"><input type="checkbox" bind:checked={draft.autostart} /> {S.settings.autostart}</label>
    <label class="inline"><input type="checkbox" bind:checked={draft.start_minimized} /> {S.settings.minimized}</label>
    <div class="row">
      <label for="theme">{S.settings.theme}</label>
      <select id="theme" bind:value={draft.theme}>
        {#each THEMES as theme (theme)}<option value={theme}>{S.settings.themes[theme]}</option>{/each}
      </select>
    </div>
  </section>

  <section class="card">
    <h2>{S.settings.cli}</h2>
    <label class="inline"><input type="checkbox" bind:checked={draft.cli_enabled} /> {S.settings.cliEnabled}</label>
    <p class="muted">{S.settings.cliHint}</p>
  </section>

  <!-- One button for the five cards above: they are saved together. -->
  <div class="buttons">
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    {#if saved}<span class="ok">{S.settings.saved}</span>{/if}
  </div>

  <section class="card">
    <h2>{S.settings.files}</h2>
    <div class="buttons">
      <button onclick={() => call(S.failed.open, commands.openFolder("Logs"))}>{S.settings.openLogs}</button>
      <button disabled={!app.settings?.zapret_dir} onclick={() => call(S.failed.open, commands.openFolder("Zapret"))}>
        {S.settings.openZapret}
      </button>
    </div>
    {#if version}<p class="muted">{S.settings.version(version)}</p>{/if}
  </section>
{/if}

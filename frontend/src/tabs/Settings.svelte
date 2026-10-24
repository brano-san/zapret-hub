<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { commands, type Info, type ProxyStatus, type Settings, type Theme } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { follow } from "$lib/draft";
  import { ms } from "$lib/format";
  import { LINKS, S } from "$lib/strings";
  import DownloadEngine from "../components/DownloadEngine.svelte";
  import Select from "../components/Select.svelte";

  const themeOptions = (["system", "light", "dark"] as Theme[]).map((theme) => ({ value: theme, label: S.settings.themes[theme] }));

  // A copy to edit, saved as a whole, and the settings it was last copied from.
  let draft = $state<Settings | null>(null);
  let from: Settings | null = null;
  let zapret2 = $state<Info | null>(null);
  let proxy = $state<ProxyStatus | null>(null);
  let saved = $state(false);
  let version = $state("");
  // The download form of the zapret2 card, opened by its own button.
  let downloader = $state<{ show: () => Promise<void> } | null>(null);

  async function pickZapret2(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path && draft) draft.zapret2_dir = path;
  }

  /** A folder found where a release usually is: into the field, then checked at once. */
  async function detectZapret2(): Promise<void> {
    const detected = await call(S.failed.detect, commands.detectFolders());
    if (!draft || !detected) return;
    if (!detected.zapret2) {
      zapret2 = { dir: null, version: null, compat: null, error: null };
      return;
    }
    draft.zapret2_dir = detected.zapret2;
    zapret2 = (await call(S.failed.zapret2, commands.checkZapret2Folder(detected.zapret2))) ?? null;
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
      zapret2_dir: draft.zapret2_dir?.trim() || null,
      socks5: draft.socks5?.trim() || null,
      nekobox_dir: draft.nekobox_dir?.trim() || null,
      // A cleared number field binds `null`: left out, the setting keeps its default.
      diag_observe_secs: draft.diag_observe_secs ?? undefined,
      diag_budget_min: draft.diag_budget_min ?? undefined,
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
      void checkZapret2();
    });
  });

  /** Edits not saved yet: the draft differs from what it was last copied from. */
  const dirty = (): boolean => draft !== null && from !== null && JSON.stringify(draft) !== JSON.stringify(from);

  onMount(() => {
    // Autostart can be switched outside the app (Task Scheduler, the autostart folder): read it again.
    void loadSettings();
    void commands.appVersion().then((v) => (version = v));
    // The tab guard asks before another tab drops the edits.
    app.unsaved = dirty;
    return () => (app.unsaved = null);
  });

  /** The window reloaded or closed with edits: the webview asks, as a tab switch does. */
  function onBeforeUnload(event: BeforeUnloadEvent): void {
    if (dirty()) event.preventDefault();
  }
</script>

<svelte:window onbeforeunload={onBeforeUnload} />

{#if draft}
  <div class="stack">
    <section class="card">
      <h2>{S.settings.zapret2}</h2>
      <p class="muted">{S.settings.zapret2Hint}</p>
      <input
        bind:value={draft.zapret2_dir}
        placeholder={app.linux ? S.settings.zapret2ExampleLinux : S.settings.zapret2Example}
        aria-label={S.settings.zapret2}
      />
      {#if zapret2?.dir}
        {#if zapret2.error}
          <pre class="bad">{zapret2.error}</pre>
        {:else}
          <p class="ok">{S.settings.zapret2Ok(zapret2.version, zapret2.compat)}</p>
        {/if}
      {:else if zapret2}
        <p class="muted">{S.settings.notFound}</p>
      {/if}
      <div class="buttons">
        <button onclick={pickZapret2}>{S.settings.pick}</button>
        <button onclick={detectZapret2}>{S.settings.detect}</button>
        <button onclick={() => downloader?.show()}>{S.download.button}</button>
        <button disabled={!app.settings?.zapret2_dir} onclick={() => call(S.failed.open, commands.openFolder("Zapret2"))}>
          {S.settings.openStrategies}
        </button>
        <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.zapret2))}>{S.settings.zapret2Releases}</button>
      </div>
      <DownloadEngine
        engine="Zapret2"
        showTrigger={false}
        bind:this={downloader}
        ondone={async (dir) => {
          if (!draft) return;
          draft.zapret2_dir = dir;
          zapret2 = (await call(S.failed.zapret2, commands.checkZapret2Folder(dir))) ?? null;
        }}
      />
    </section>

    <section class="card">
      <h2>{S.settings.filters}</h2>
      <label class="inline"><input type="checkbox" bind:checked={draft.game_filter} /> {S.settings.gameFilter}</label>
      <p class="muted">{S.settings.gameFilterHint}</p>
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
        <span class="muted">{S.settings.theme}</span>
        <Select bind:value={draft.theme} options={themeOptions} ariaLabel={S.settings.theme} />
      </div>
    </section>

    <section class="card">
      <h2>{S.settings.cli}</h2>
      <label class="inline"><input type="checkbox" bind:checked={draft.cli_enabled} /> {S.settings.cliEnabled}</label>
      <p class="muted">{app.linux ? S.settings.cliHintLinux : S.settings.cliHint}</p>
    </section>

    <section class="card">
      <h2>{S.settings.diag}</h2>
      <div class="row">
        <label for="diag-observe">{S.settings.diagObserve}</label>
        <input id="diag-observe" type="number" min="10" max="120" bind:value={draft.diag_observe_secs} />
      </div>
      <div class="row">
        <label for="diag-budget">{S.settings.diagBudget}</label>
        <input id="diag-budget" type="number" min="1" max="60" bind:value={draft.diag_budget_min} />
      </div>
      <div class="row">
        <label for="nekobox">{S.settings.nekobox}</label>
        <input id="nekobox" class="grow" bind:value={draft.nekobox_dir} />
      </div>
      <p class="muted">{S.settings.nekoboxHint}</p>
    </section>

    <!-- One button for the cards above: they are saved together. -->
    <div class="buttons save">
      <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
      {#if saved}<span class="ok">{S.settings.saved}</span>{/if}
    </div>

    <section class="card">
      <h2>{S.settings.about}</h2>
      {#if version}<p class="muted">{S.settings.version(version)}</p>{/if}
      <div class="buttons">
        <button onclick={() => call(S.failed.open, commands.openFolder("Logs"))}>{S.settings.openLogs}</button>
      </div>
    </section>
  </div>
{/if}

<style>
  /* One vertical rhythm: an even gap between the cards and inside them. */
  .stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .stack :global(.card) {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .stack :global(.card p),
  .stack :global(.card h2),
  .stack :global(.card .buttons) {
    margin: 0;
  }
</style>

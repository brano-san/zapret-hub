<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type Candidates, type FolderCheck, type Info } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings, refreshOverview } from "$lib/app.svelte";
  import { foldersReady } from "$lib/format";
  import { LINKS, S } from "$lib/strings";
  import DownloadEngine from "./components/DownloadEngine.svelte";

  let step = $state(1);
  let candidates = $state<Candidates | null>(null);
  let flowsealPath = $state("");
  /** The check of `flowsealPath` as it was when checked; `null` while the field is empty. */
  let folder = $state<FolderCheck | null>(null);
  let checking1 = $state(false);
  let zapret2Path = $state("");
  /** The check of `zapret2Path` as it was when checked; `null` while the field is empty. */
  let zapret2 = $state<Info | null>(null);
  let checking2 = $state(false);
  let detecting = $state(true);
  let found1 = $state(false);
  let found2 = $state(false);
  let socks5 = $state("");
  let autostart = $state(true);
  let minimized = $state(true);

  async function checkFlowseal(): Promise<void> {
    const path = flowsealPath.trim();
    found1 = false;
    if (!path) {
      folder = null;
      return;
    }
    checking1 = true;
    folder = (await call(S.failed.folder, commands.checkFolder(path))) ?? null;
    checking1 = false;
  }

  async function checkZapret2(): Promise<void> {
    const path = zapret2Path.trim();
    found2 = false;
    if (!path) {
      zapret2 = null;
      return;
    }
    checking2 = true;
    zapret2 = (await call(S.failed.zapret2, commands.checkZapret2Folder(path))) ?? null;
    checking2 = false;
  }

  onMount(() => {
    // The folders come first and fill the fields; the proxies probe ports and may take longer.
    void (async () => {
      const found = await call(S.failed.wizard, commands.detectFolders());
      detecting = false;
      // What the user typed or picked meanwhile stays.
      if (found?.flowseal && !flowsealPath.trim()) {
        flowsealPath = found.flowseal.path;
        folder = found.flowseal;
        found1 = true;
      }
      if (found?.zapret2 && !zapret2Path.trim()) {
        zapret2Path = found.zapret2;
        await checkZapret2();
        found2 = zapret2Path === found.zapret2;
      }
    })();
    void (async () => {
      candidates = (await call(S.failed.wizard, commands.wizardCandidates())) ?? { folders: [], proxies: [] };
      socks5 = candidates.proxies[0] ?? "";
    })();
  });

  async function pickFlowseal(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path) {
      flowsealPath = path;
      await checkFlowseal();
    }
  }

  async function pickZapret2(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path) {
      zapret2Path = path;
      await checkZapret2();
    }
  }

  // A path typed and not checked yet holds the step back like a failed one.
  const zapret2Checked = $derived((zapret2?.dir ?? "") === zapret2Path.trim());
  const flowsealChecked = $derived((folder?.path ?? "") === flowsealPath.trim());
  const ready = $derived(!checking1 && !checking2 && flowsealChecked && zapret2Checked && foldersReady(folder, zapret2));

  async function finish(): Promise<void> {
    if (!app.settings) return;
    const settings = {
      ...app.settings,
      zapret_dir: folder?.path ?? null,
      zapret2_dir: zapret2?.dir ?? null,
      socks5: socks5.trim() || null,
      autostart,
      start_minimized: minimized,
      wizard_done: true,
    };
    if ((await change(S.failed.wizard, () => commands.saveSettings(settings))) === undefined) return;
    await loadSettings();
    // The overview from before the wizard still says the folder is not set.
    await refreshOverview();
  }
</script>

<div class="wizard card">
  <h1>{S.wizard.title}</h1>
  <p class="muted">{S.wizard.step(step, 3)}</p>

  {#if step === 1}
    <h2>{S.wizard.folderTitle}</h2>
    <p>{S.wizard.folderText}</p>
    <div class="engines">
      <section>
        <h3>{S.wizard.flowsealTitle}</h3>
        <p class="muted">{S.wizard.flowsealText}</p>
        <div class="row">
          <input
            class="grow"
            bind:value={flowsealPath}
            onchange={checkFlowseal}
            placeholder={S.settings.folderHint}
            aria-label={S.wizard.flowsealTitle}
          />
          <button onclick={pickFlowseal}>{S.settings.pick}</button>
        </div>
        {#if checking1}
          <p class="muted">{S.common.checking}</p>
        {:else if folder}
          <p class={folder.missing.length > 0 ? "bad" : "ok"}>
            {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
          </p>
        {:else if detecting}
          <p class="muted">{S.wizard.searching}</p>
        {/if}
        {#if found1 && !checking1}<p class="muted">{S.wizard.autoFound}</p>{/if}
        <DownloadEngine
          engine="Flowseal"
          ondone={async (dir) => {
            flowsealPath = dir;
            await checkFlowseal();
          }}
        />
      </section>
      <section>
        <h3>{S.wizard.zapret2Title}</h3>
        <p class="muted">{S.wizard.zapret2Text}</p>
        <div class="row">
          <input
            class="grow"
            bind:value={zapret2Path}
            onchange={checkZapret2}
            placeholder={S.settings.zapret2Example}
            aria-label={S.settings.zapret2}
          />
          <button onclick={pickZapret2}>{S.settings.pick}</button>
        </div>
        {#if checking2}
          <p class="muted">{S.common.checking}</p>
        {:else if detecting && !zapret2?.dir}
          <p class="muted">{S.wizard.searching}</p>
        {:else if zapret2?.dir}
          {#if zapret2.error}
            <pre class="bad">{zapret2.error}</pre>
          {:else}
            <p class="ok">{S.settings.zapret2Ok(zapret2.version, zapret2.compat)}</p>
          {/if}
        {/if}
        {#if found2 && !checking2}<p class="muted">{S.wizard.autoFound}</p>{/if}
        <DownloadEngine
          engine="Zapret2"
          ondone={async (dir) => {
            zapret2Path = dir;
            await checkZapret2();
          }}
        />
        <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.zapret2))}>
          {S.settings.zapret2Releases}
        </button>
      </section>
    </div>
    {#if !folder && zapret2?.dir && !zapret2.error}<p class="warn">{S.wizard.zapret2Alone}</p>{/if}
    {#if !ready && !checking1 && !checking2 && !detecting}<p class="muted">{S.wizard.needOne}</p>{/if}
    <div class="buttons">
      <span class="grow"></span>
      <button class="primary" disabled={!ready} onclick={() => (step = 2)}>
        {S.wizard.next}
      </button>
    </div>
  {:else if step === 2}
    <h2>{S.wizard.proxyTitle}</h2>
    <p>{S.wizard.proxyText}</p>
    {#each candidates?.proxies ?? [] as addr (addr)}
      <label class="choice">
        <input type="radio" name="proxy" checked={socks5 === addr} onchange={() => (socks5 = addr)} />
        {addr}
      </label>
    {/each}
    {#if candidates && candidates.proxies.length === 0}<p class="muted">{S.wizard.noProxies}</p>{/if}
    <div class="row">
      <input class="grow" bind:value={socks5} placeholder="127.0.0.1:2080" aria-label={S.settings.proxyAddress} />
    </div>
    <div class="buttons">
      <button onclick={() => (step = 1)}>{S.wizard.back}</button>
      <span class="grow"></span>
      <button
        onclick={() => {
          socks5 = "";
          step = 3;
        }}>{S.wizard.noProxy}</button
      >
      <button class="primary" onclick={() => (step = 3)}>{S.wizard.next}</button>
    </div>
  {:else}
    <h2>{S.wizard.autostartTitle}</h2>
    <label class="inline"><input type="checkbox" bind:checked={autostart} /> {S.settings.autostart}</label>
    <label class="inline"><input type="checkbox" bind:checked={minimized} /> {S.settings.minimized}</label>
    <div class="buttons">
      <button onclick={() => (step = 2)}>{S.wizard.back}</button>
      <span class="grow"></span>
      <button class="primary" disabled={app.busy} onclick={finish}>{S.wizard.done}</button>
    </div>
  {/if}
</div>

<style>
  .wizard {
    max-width: 860px;
    margin: 48px auto;
  }

  /* The two engines side by side, one under the other in a narrow window. */
  .engines {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 14px 24px;
  }

  .engines h3 {
    margin-top: 0;
  }
</style>

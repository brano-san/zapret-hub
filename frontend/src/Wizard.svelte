<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type Candidates, type FolderCheck, type Info } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { foldersReady } from "$lib/format";
  import { LINKS, S } from "$lib/strings";

  let step = $state(1);
  let candidates = $state<Candidates | null>(null);
  let folder = $state<FolderCheck | null>(null);
  let zapret2Path = $state("");
  /** The check of `zapret2Path` as it was when checked; `null` while the field is empty. */
  let zapret2 = $state<Info | null>(null);
  let checking2 = $state(false);
  let socks5 = $state("");
  let autostart = $state(true);
  let minimized = $state(true);

  onMount(async () => {
    candidates = (await call(S.failed.wizard, commands.wizardCandidates())) ?? { folders: [], proxies: [] };
    folder = candidates.folders.find((found) => found.missing.length === 0) ?? null;
    socks5 = candidates.proxies[0] ?? "";
  });

  async function pick(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path) folder = (await call(S.failed.folder, commands.checkFolder(path))) ?? null;
  }

  async function checkZapret2(): Promise<void> {
    const path = zapret2Path.trim();
    if (!path) {
      zapret2 = null;
      return;
    }
    checking2 = true;
    zapret2 = (await call(S.failed.zapret2, commands.checkZapret2Folder(path))) ?? null;
    checking2 = false;
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
  const ready = $derived(!checking2 && zapret2Checked && foldersReady(folder, zapret2));

  /** «Настроить позже» keeps the defaults but does not ask again. */
  async function finish(later: boolean): Promise<void> {
    if (!app.settings) return;
    const settings = later
      ? { ...app.settings, wizard_done: true }
      : {
          ...app.settings,
          zapret_dir: folder?.path ?? null,
          zapret2_dir: zapret2?.dir ?? null,
          socks5: socks5.trim() || null,
          autostart,
          start_minimized: minimized,
          wizard_done: true,
        };
    if ((await change(S.failed.wizard, () => commands.saveSettings(settings))) !== undefined) await loadSettings();
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
        {#if !candidates}<p class="muted">{S.wizard.searching}</p>{/if}
        {#each candidates?.folders ?? [] as found}
          <label class="choice">
            <input
              type="radio"
              name="folder"
              checked={folder?.path === found.path}
              disabled={found.missing.length > 0}
              onchange={() => (folder = found)}
            />
            <span>{found.path}</span>
            <span class={found.missing.length > 0 ? "bad" : "muted"}>
              {found.missing.length > 0 ? S.settings.missing(found.missing) : S.settings.folderOk(found.version)}
            </span>
          </label>
        {/each}
        <label class="choice">
          <input type="radio" name="folder" checked={folder === null} onchange={() => (folder = null)} />
          <span>{S.wizard.noFlowseal}</span>
        </label>
        {#if folder && !candidates?.folders.some((found) => found.path === folder?.path)}
          <p>
            {folder.path} —
            <span class={folder.missing.length > 0 ? "bad" : "ok"}>
              {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
            </span>
          </p>
        {/if}
        <div class="buttons"><button onclick={pick}>{S.wizard.pickOther}</button></div>
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
        {:else if zapret2?.dir}
          {#if zapret2.error}
            <pre class="bad">{zapret2.error}</pre>
          {:else}
            <p class="ok">{S.settings.zapret2Ok(zapret2.version, zapret2.compat)}</p>
          {/if}
        {/if}
        <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.zapret2))}>
          {S.settings.zapret2Releases}
        </button>
      </section>
    </div>
    {#if !folder && zapret2?.dir && !zapret2.error}<p class="warn">{S.wizard.zapret2Alone}</p>{/if}
    {#if !ready && !checking2}<p class="muted">{S.wizard.needOne}</p>{/if}
    <div class="buttons">
      <span class="grow"></span>
      <button class="link" onclick={() => finish(true)}>{S.wizard.later}</button>
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
      <button class="primary" disabled={app.busy} onclick={() => finish(false)}>{S.wizard.done}</button>
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

<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type Candidates, type FolderCheck } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  let step = $state(1);
  let candidates = $state<Candidates | null>(null);
  let folder = $state<FolderCheck | null>(null);
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

  /** «Настроить позже» keeps the defaults but does not ask again. */
  async function finish(later: boolean): Promise<void> {
    if (!app.settings) return;
    const settings = later
      ? { ...app.settings, wizard_done: true }
      : {
          ...app.settings,
          zapret_dir: folder?.path ?? null,
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
    {#if folder && !candidates?.folders.some((found) => found.path === folder?.path)}
      <p>
        {folder.path} —
        <span class={folder.missing.length > 0 ? "bad" : "ok"}>
          {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
        </span>
      </p>
    {/if}
    <div class="buttons">
      <button onclick={pick}>{S.wizard.pickOther}</button>
      <span class="grow"></span>
      <button class="link" onclick={() => finish(true)}>{S.wizard.later}</button>
      <button class="primary" disabled={!folder || folder.missing.length > 0} onclick={() => (step = 2)}>
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
    max-width: 640px;
    margin: 48px auto;
  }
</style>

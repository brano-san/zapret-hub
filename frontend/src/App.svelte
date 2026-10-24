<script lang="ts">
  import { onMount } from "svelte";
  import Diag from "./tabs/Diag.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import Notices from "./components/Notices.svelte";
  import Overview from "./tabs/Overview.svelte";
  import Routes from "./tabs/Routes.svelte";
  import Settings from "./tabs/Settings.svelte";
  import Strategies from "./tabs/Strategies.svelte";
  import Tests from "./tabs/Tests.svelte";
  import Zapret from "./tabs/Zapret.svelte";
  import Wizard from "./Wizard.svelte";
  import { app, refreshOverview, start, type Tab } from "$lib/app.svelte";
  import { events } from "$lib/ipc/bindings";
  import { S } from "$lib/strings";

  const TABS = [
    { id: "overview", label: S.tabs.overview, view: Overview },
    { id: "strategies", label: S.tabs.strategies, view: Strategies },
    { id: "tests", label: S.tabs.tests, view: Tests },
    { id: "zapret", label: S.tabs.zapret, view: Zapret },
    { id: "routes", label: S.tabs.routes, view: Routes },
    { id: "diag", label: S.tabs.diag, view: Diag },
    { id: "settings", label: S.tabs.settings, view: Settings },
  ] satisfies { id: Tab; label: string; view: unknown }[];

  const current = $derived(TABS.find((tab) => tab.id === app.tab) ?? TABS[0]!);

  /** The tray's «Диагностика…»: `OpenTab` to an open page, `#diag` to a new one. */
  function openTab(tab: string): void {
    const wanted = TABS.find((item) => item.id === tab);
    if (wanted) void app.setTab(wanted.id);
  }

  onMount(() => {
    openTab(location.hash.slice(1));
    void events.openTab.listen(({ payload }) => openTab(payload.tab));
    void start();
  });

  // Files dropped into a strategies folder by hand announce nothing: coming back to the window reads everything again.
  function onFocus(): void {
    app.revision++;
    void refreshOverview();
  }
</script>

<svelte:window onfocus={onFocus} />

{#if app.settings && !app.settings.wizard_done}
  <Wizard />
{:else if app.settings}
  <div class="shell">
    <nav>
      {#each TABS as tab (tab.id)}
        <button class="tab" class:active={tab.id === current.id} onclick={() => app.setTab(tab.id)}>{tab.label}</button>
      {/each}
      <div class="nav-controls">
        {#if app.live}<span class="testing warn">{S.overview.testing}</span>{/if}
        {#if app.diagLive}<span class="testing warn">{S.diag.running}</span>{/if}
      </div>
    </nav>
    <main>
      <current.view />
    </main>
  </div>
{:else}
  <!-- The settings have not arrived (IPC broken or slow): the page answers for itself instead of white -->
  <p class="loading">{S.common.loading}</p>
{/if}

<Notices />
<ConfirmDialog />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  nav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 16px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }


  .tab {
    border: none;
    background: none;
  }

  .tab.active {
    background: var(--hover);
    font-weight: 600;
  }

  .nav-controls {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .loading {
    margin: 40vh auto 0;
    text-align: center;
    color: var(--muted);
  }

  .testing {
    font-size: 13px;
  }

  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px;
    overflow: auto;
  }
</style>

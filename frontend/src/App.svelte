<script lang="ts">
  import { onMount } from "svelte";
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
  import { S } from "$lib/strings";

  const TABS = [
    { id: "overview", label: S.tabs.overview, view: Overview },
    { id: "strategies", label: S.tabs.strategies, view: Strategies },
    { id: "tests", label: S.tabs.tests, view: Tests },
    { id: "zapret", label: S.tabs.zapret, view: Zapret },
    { id: "routes", label: S.tabs.routes, view: Routes },
    { id: "settings", label: S.tabs.settings, view: Settings },
  ] satisfies { id: Tab; label: string; view: unknown }[];

  const current = $derived(TABS.find((tab) => tab.id === app.tab) ?? TABS[0]!);

  onMount(() => void start());

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
        <button class="tab" class:active={tab.id === current.id} onclick={() => (app.tab = tab.id)}>{tab.label}</button>
      {/each}
      {#if app.live}<span class="testing warn">{S.overview.testing}</span>{/if}
    </nav>
    <main>
      <current.view />
    </main>
  </div>
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

  .testing {
    margin-left: auto;
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

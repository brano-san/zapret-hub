<script lang="ts">
  import { onMount } from "svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import Notices from "./components/Notices.svelte";
  import Overview from "./tabs/Overview.svelte";
  import Settings from "./tabs/Settings.svelte";
  import Wizard from "./Wizard.svelte";
  import { app, start, type Tab } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  const TABS = [
    { id: "overview", label: S.tabs.overview, view: Overview },
    // Tasks 18–20 add Strategies, Tests and Zapret here, in that order.
    { id: "settings", label: S.tabs.settings, view: Settings },
  ] satisfies { id: Tab; label: string; view: unknown }[];

  const current = $derived(TABS.find((tab) => tab.id === app.tab) ?? TABS[0]!);

  onMount(() => void start());
</script>

{#if app.settings && !app.settings.wizard_done}
  <Wizard />
{:else if app.settings}
  <div class="shell">
    <nav>
      <span class="brand">{S.app}</span>
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

  .brand {
    margin-right: 16px;
    font-weight: 600;
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

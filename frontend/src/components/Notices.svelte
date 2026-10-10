<script lang="ts">
  import { notices } from "$lib/notices.svelte";
  import { S } from "$lib/strings";

  async function copy(title: string, text: string): Promise<void> {
    await navigator.clipboard.writeText(text ? `${title}\n\n${text}` : title);
  }
</script>

{#if notices.current}
  {@const notice = notices.current}
  <aside class="notice" class:error={notice.level === "Error"} role="alert">
    <div class="head">
      <strong>{notice.title}</strong>
      {#if notices.items.length > 1}<span class="muted">{S.notices.more(notices.items.length - 1)}</span>{/if}
    </div>
    {#if notice.text}<pre>{notice.text}</pre>{/if}
    <div class="buttons">
      <button onclick={() => copy(notice.title, notice.text)}>{S.common.copy}</button>
      <button class="primary" onclick={() => notices.dismiss()}>{S.common.close}</button>
    </div>
  </aside>
{/if}

<style>
  .notice {
    position: fixed;
    right: 16px;
    bottom: 16px;
    width: min(520px, calc(100vw - 32px));
    padding: 12px 14px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 4px solid var(--accent);
    border-radius: 8px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    z-index: 20;
  }

  .notice.error {
    border-left-color: var(--bad);
  }

  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
</style>

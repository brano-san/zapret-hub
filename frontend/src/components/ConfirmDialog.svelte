<script lang="ts">
  import { confirmation } from "$lib/confirm.svelte";
  import { S } from "$lib/strings";

  let cancelButton = $state<HTMLButtonElement>();

  // Focus leaves the button behind the backdrop: Enter or Space must not press it again.
  $effect(() => {
    if (confirmation.question) cancelButton?.focus();
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && confirmation.question) confirmation.answer(false);
  }
</script>

<svelte:window {onkeydown} />

{#if confirmation.question}
  {@const question = confirmation.question}
  <div class="backdrop">
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" tabindex="-1">
      <h2 id="confirm-title">{question.title}</h2>
      <pre>{question.text}</pre>
      <div class="buttons">
        <button bind:this={cancelButton} onclick={() => confirmation.answer(false)}>{S.common.cancel}</button>
        <button class={question.danger ? "danger" : "primary"} onclick={() => confirmation.answer(true)}>
          {question.yes}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.4);
    z-index: 30;
  }

  .dialog {
    width: min(560px, calc(100vw - 32px));
    padding: 16px 18px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
</style>

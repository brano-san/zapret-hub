<script lang="ts">
  import { S } from "$lib/strings";

  interface Props {
    /** What to copy; `undefined` when it could not be had (its error is already shown). */
    text: () => Promise<string | undefined> | string;
  }

  let { text }: Props = $props();

  let copied = $state(false);
  let error = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy(): Promise<void> {
    error = "";
    const value = await text();
    if (value === undefined) return;
    try {
      await navigator.clipboard.writeText(value);
    } catch (err) {
      copied = false;
      error = S.common.copyFailed(String(err));
      return;
    }
    copied = true;
    clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 1500);
  }
</script>

<button onclick={copy}>{copied ? S.common.copiedMark : S.common.copy}</button>
{#if error}<span class="bad">{error}</span>{/if}

<script lang="ts">
  import { REPEATS } from "$lib/format";
  import { S } from "$lib/strings";

  /** Passes over the list for the next run (`Request.repeats`). */
  let { value = $bindable(1), disabled = false }: { value?: number; disabled?: boolean } = $props();
</script>

<label class="inline" title={S.tests.repeatsHint}>
  {S.tests.repeats}
  <select bind:value {disabled}>
    {#each REPEATS as n (n)}<option value={n}>{n}</option>{/each}
  </select>
</label>

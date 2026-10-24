<script lang="ts">
  import { REPEATS } from "$lib/format";
  import { S } from "$lib/strings";
  import Select from "./Select.svelte";

  /** Passes over the list for the next run (`Request.repeats`). */
  let { value = $bindable(1), disabled = false }: { value?: number; disabled?: boolean } = $props();
</script>

<label class="inline" title={S.tests.repeatsHint}>
  {S.tests.repeats}
  <Select bind:value options={[...REPEATS]} {disabled} ariaLabel={S.tests.repeats} />
</label>

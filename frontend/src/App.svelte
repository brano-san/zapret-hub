<script lang="ts">
  import { commands, type Overview } from "$lib/ipc/bindings";

  let overview = $state<Overview | null>(null);
  let error = $state("");
  void commands.getOverview().then((answer) => {
    if (answer.status === "ok") overview = answer.data;
    else error = answer.error;
  });
</script>

<main>
  <h1>Zapret Hub</h1>
  {#if error}<p class="bad">{error}</p>{/if}
  <pre>{JSON.stringify(overview, null, 2)}</pre>
</main>

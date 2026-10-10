<script lang="ts">
  import { commands } from "$lib/ipc/bindings";

  let version = $state("");
  void commands.appVersion().then((v) => (version = v));
</script>

<main>
  <h1>Zapret Hub</h1>
  <p class="muted">{version}</p>
</main>

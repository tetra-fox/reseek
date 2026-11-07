<script lang="ts">
  import { Dialog } from "bits-ui";

  /** comes and goes without fading, for what the keys open many times a day */
  let { instant = false }: { instant?: boolean } = $props();
</script>

<Dialog.Overlay
  class={[
    "fixed inset-0 z-50 bg-scrim",
    !instant && "data-[state=closed]:animate-fade-out data-[state=open]:animate-fade-in"
  ]} />

<script lang="ts">
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import type { Topic } from "#lib/live/live.svelte.js";

  // a screen that keeps itself current, for the keepCurrent tests
  let { reload, topic }: { reload: () => void; topic: Topic } = $props();
  keepCurrent(
    () => reload(),
    () => topic
  );
</script>

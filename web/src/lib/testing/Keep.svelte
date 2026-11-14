<script lang="ts">
  import type { Freeze } from "#lib/lists/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import type { Topic } from "#lib/live/live.svelte.js";

  // a screen that keeps itself current, for the keepCurrent tests, with its list's freeze
  let { reload, topic, freeze }: { reload: () => void; topic: Topic; freeze?: Freeze } = $props();
  keepCurrent(
    () => reload(),
    () => topic,
    // svelte-ignore state_referenced_locally
    freeze
  );
</script>

<script lang="ts">
  import { clock } from "#lib/clock.svelte.js";
  import { ago, dateTime } from "#lib/format.js";

  // a moment as "3 minutes ago", kept current, the exact time on hover
  let { at, class: className }: { at: string; class?: string } = $props();
</script>

<time datetime={at} title={dateTime(at)} class={className}>{ago(at, clock.now)}</time>

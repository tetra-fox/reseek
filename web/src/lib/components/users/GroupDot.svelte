<script lang="ts">
  import { groupColor } from "#lib/users/colors.js";

  // a group's color as a dot, nothing for a group without one
  let { color, class: className }: { color: string | null | undefined; class?: string } = $props();

  const shown = $derived(groupColor(color));
</script>

{#if shown}
  <span
    aria-hidden="true"
    class={["size-2 shrink-0 rounded-full", className]}
    style:background-color={shown}></span>
{/if}

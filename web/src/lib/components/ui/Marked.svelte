<script lang="ts">
  import type { SnippetPart } from "#lib/text.js";

  // text from a search with what matched marked, always rendered as text
  let { parts }: { parts: SnippetPart[] } = $props();
</script>

<!-- the mark's padding is taken back by its margin, so a mark in a word doesn't split it -->
{#each parts as part (part.at)}{#if part.hit}<mark
      class="rounded-sm -mx-px bg-warning-soft px-px text-fg">{part.text}</mark
    >{:else}{part.text}{/if}{/each}

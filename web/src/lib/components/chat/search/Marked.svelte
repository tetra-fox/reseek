<script lang="ts">
  import type { SnippetPart } from "#lib/text.js";

  // text from a search with what matched marked, always rendered as text
  let { parts }: { parts: SnippetPart[] } = $props();
</script>

{#each parts as part (part.at)}{#if part.hit}<mark class="rounded-sm bg-warning-soft px-0.5 text-fg"
      >{part.text}</mark
    >{:else}{part.text}{/if}{/each}

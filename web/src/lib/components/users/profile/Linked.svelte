<script lang="ts">
  import { linkify } from "#lib/text.js";

  // text from another user, shown as text, with its http(s) links clickable
  let { text }: { text: string } = $props();

  const parts = $derived(linkify(text));
</script>

{#each parts as part (part.at)}{#if part.url}<a
      href={part.url}
      target="_blank"
      rel="noopener noreferrer"
      class="text-accent underline-offset-2 hover:underline">{part.text}</a
    >{:else}{part.text}{/if}{/each}

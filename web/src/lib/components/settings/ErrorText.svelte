<script lang="ts">
  // an error as the daemon writes it. One of several lines, such as a TOML parse error, keeps its
  // lines in a fixed-width font, so its caret stays under the column it points at
  let { text }: { text: string } = $props();
</script>

{#if text.includes("\n")}
  <span class="mt-1 block font-mono text-xs wrap-anywhere whitespace-pre-wrap">{text}</span>
{:else}
  <span class="wrap-anywhere">{text}</span>
{/if}

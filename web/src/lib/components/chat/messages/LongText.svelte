<script lang="ts">
  import { LONG } from "#lib/chat/messages/messages.js";

  import ChatText from "./ChatText.svelte";

  // a message's text, cut after LONG characters until the rest is asked for
  let { text, watch }: { text: string; watch: string[] } = $props();

  let unfolded = $state(false);
  const long = $derived(text.length > LONG && !unfolded);
</script>

<!-- no space before the button: callers wrap the text in a pre-wrap box, which keeps one -->
<ChatText text={long ? `${text.slice(0, LONG)}…` : text} {watch} />{#if long}<button
    type="button"
    class="ml-1 text-xs text-accent hover:underline"
    onclick={() => (unfolded = true)}>Show more</button
  >{/if}

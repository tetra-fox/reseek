<script lang="ts">
  import { LONG } from "#lib/chat/messages/messages.js";
  import { fold } from "#lib/text.js";

  import ChatText from "./ChatText.svelte";

  // a message's text, cut after LONG characters until the rest is asked for
  let { text, watch }: { text: string; watch: string[] } = $props();

  let unfolded = $state(false);
  const folded = $derived(unfolded ? null : fold(text, LONG));
</script>

<!-- no space before the button: callers wrap the text in a pre-wrap box, which keeps one -->
<ChatText text={folded === null ? text : `${folded}…`} {watch} />{#if folded !== null}<button
    type="button"
    class="ml-1 text-xs text-accent hover:underline"
    onclick={() => (unfolded = true)}>Show more</button
  >{/if}

<script lang="ts" module>
  import { app } from "#lib/app.svelte.js";
  import { formatCount } from "#lib/format.js";

  /** The most characters the daemon sends in one message, its `chat.max_message_length` */
  export const maxLength = (): number => {
    const configured = app.option("chat.max_message_length");
    return typeof configured === "number" && configured > 0 ? configured : 2000;
  };

  /** Whether a message is close enough to the most to show its length */
  export const nearMax = (length: number, max: number) => length > max * 0.9;
</script>

<script lang="ts">
  // a message's length against the most it can have, red past it
  let { length, max }: { length: number; max: number } = $props();
</script>

<span class={["ml-auto tabular-nums", length > max ? "text-danger" : "text-fg-muted"]}>
  {formatCount(length)}/{formatCount(max)}
</span>

<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";

  // a user's presence as a dot: green online, yellow away, gray offline or unknown
  let { presence }: { presence: Schemas["UserPresence"] | null | undefined } = $props();
</script>

<span
  class={[
    "inline-block size-2 shrink-0 rounded-full",
    presence === "Online" ? "bg-success" : presence === "Away" ? "bg-warning" : "bg-fg-subtle/50"
  ]}
  title={presence ?? "Unknown"}></span>

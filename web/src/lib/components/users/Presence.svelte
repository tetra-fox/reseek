<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  type Props = {
    presence: Schemas["UserPresence"] | null | undefined;
    /** what its tooltip says instead of the presence in words */
    text?: string;
    /** the presence is in words beside the dot, which then needs no tooltip */
    worded?: boolean;
    /** its tooltip waits for its row to wake, as `Tooltip`'s `dormant` */
    dormant?: boolean;
  };

  // a user's presence as a dot: green online, yellow away, gray offline or unknown
  let { presence, text, worded = false, dormant = false }: Props = $props();
</script>

<Tooltip text={worded ? undefined : (text ?? presence ?? "Unknown")} dense spoken {dormant}>
  {#snippet child({ props })}
    <span
      {...props}
      class={[
        "inline-block size-2 shrink-0 rounded-full",
        presence === "Online"
          ? "bg-success"
          : presence === "Away"
            ? "bg-warning"
            : "bg-fg-subtle/50"
      ]}></span>
  {/snippet}
</Tooltip>

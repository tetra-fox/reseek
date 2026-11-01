<script lang="ts">
  import BellOffIcon from "@lucide/svelte/icons/bell-off";

  import { notifier } from "#lib/chat/notify.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { DESKTOP_NEEDS_HTTPS, TURN_ON_NOTIFICATIONS } from "#lib/wording.js";

  // shown where an option asks for desktop notifications, says why this browser shows none
  const blocked = $derived(
    {
      granted: null,
      default: "Desktop notifications are not turned on in this browser.",
      denied:
        "This browser blocks desktop notifications from this site. Allow them in its settings.",
      insecure: DESKTOP_NEEDS_HTTPS,
      unsupported: "This browser does not support desktop notifications."
    }[notifier.permission]
  );
</script>

{#if blocked}
  <!-- the text keeps the line's width and the button goes under it when both don't fit -->
  <div
    class="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-control border border-warning/40 bg-warning-soft px-3 py-2 text-sm">
    <span class="flex min-w-[min(100%,16rem)] flex-1 items-center gap-2"
      ><BellOffIcon class="size-3.5 shrink-0" />{blocked}</span>
    {#if notifier.permission === "default"}
      <Button variant="link" onclick={() => void notifier.ask()}>{TURN_ON_NOTIFICATIONS}</Button>
    {/if}
  </div>
{/if}

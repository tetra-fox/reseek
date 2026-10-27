<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CircleHelpIcon from "@lucide/svelte/icons/circle-help";
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import ClockIcon from "@lucide/svelte/icons/clock";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
  import XIcon from "@lucide/svelte/icons/x";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a search's state as an icon, the state's own words in its tooltip
  let { state }: { state: string } = $props();

  const icon = $derived(
    state === "None" || state === "Queued" || state === "Requested"
      ? { Icon: ClockIcon, tone: "text-fg-muted", spin: false }
      : state === "InProgress"
        ? { Icon: LoaderCircleIcon, tone: "text-success", spin: true }
        : state === "Completed, Cancelled"
          ? { Icon: CircleStopIcon, tone: "text-success", spin: false }
          : state === "Completed, Errored"
            ? { Icon: XIcon, tone: "text-danger", spin: false }
            : state.startsWith("Completed, ")
              ? { Icon: CheckIcon, tone: "text-success", spin: false }
              : { Icon: CircleHelpIcon, tone: "text-warning", spin: false }
  );
</script>

<Tooltip text={state} hint>
  {#snippet child({ props })}
    <span {...props} class="inline-flex">
      <icon.Icon
        class={["size-4", icon.tone, icon.spin && "animate-spin motion-reduce:animate-none"]}
        aria-hidden="true" />
    </span>
  {/snippet}
</Tooltip>

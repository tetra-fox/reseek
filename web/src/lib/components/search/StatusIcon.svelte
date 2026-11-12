<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CircleHelpIcon from "@lucide/svelte/icons/circle-help";
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import ClockIcon from "@lucide/svelte/icons/clock";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
  import XIcon from "@lucide/svelte/icons/x";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { type SearchStage, searchStage, type SearchState } from "#lib/search/searches.svelte.js";

  // a search's state as an icon, the state's own words in its tooltip
  let { state }: { state: SearchState } = $props();

  const ICONS: Record<SearchStage, { Icon: typeof CheckIcon; tone: string; spin: boolean }> = {
    waiting: { Icon: ClockIcon, tone: "text-fg-muted", spin: false },
    running: { Icon: LoaderCircleIcon, tone: "text-success", spin: true },
    found: { Icon: CheckIcon, tone: "text-success", spin: false },
    cancelled: { Icon: CircleStopIcon, tone: "text-success", spin: false },
    errored: { Icon: XIcon, tone: "text-danger", spin: false },
    unknown: { Icon: CircleHelpIcon, tone: "text-warning", spin: false }
  };

  const icon = $derived(ICONS[searchStage(state)]);
</script>

<Tooltip text={state} dense spoken>
  {#snippet child({ props })}
    <span {...props} class="inline-flex">
      <icon.Icon
        class={["size-4", icon.tone, icon.spin && "animate-spin motion-reduce:animate-none"]}
        aria-hidden="true" />
    </span>
  {/snippet}
</Tooltip>

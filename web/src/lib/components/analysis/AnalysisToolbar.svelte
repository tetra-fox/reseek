<script lang="ts">
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import CircleHelpIcon from "@lucide/svelte/icons/circle-help";
  import ListChecksIcon from "@lucide/svelte/icons/list-checks";
  import { Popover } from "bits-ui";
  import type { Component, Snippet } from "svelte";

  import type { Report } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import { spectrogramView } from "#lib/analysis/spectrogram/view.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";

  import AnalysisGuide from "./AnalysisGuide.svelte";
  import AnalysisJobs from "./AnalysisJobs.svelte";
  import CompareTable from "./CompareTable.svelte";

  // the strip between the two rows: what acts on both files, with the comparison, the queue and
  // the guide in popovers
  type Props = {
    /** the two reports to compare, null until both slots hold a current one */
    compared: { a: Report; b: Report } | null;
  };

  let { compared }: Props = $props();

  $effect(() => jobs.retain());
  const both = $derived(!!slots.a && !!slots.b);
</script>

{#snippet panel(label: string, Icon: Component, content: Snippet, disabled = false, count = 0)}
  <Popover.Root>
    <Popover.Trigger {disabled}>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="sm">
          <Icon />{label}
          {#if count}<Badge class="tabular-nums">{count}</Badge>{/if}
        </Button>
      {/snippet}
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content
        sideOffset={6}
        collisionPadding={8}
        class="z-50 w-[min(30rem,calc(100vw-2rem))] rounded-card border bg-overlay p-3 shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
        {@render content()}
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>
{/snippet}

{#snippet comparison()}
  {#if compared}<CompareTable a={compared.a} b={compared.b} />{/if}
{/snippet}
{#snippet queue()}<AnalysisJobs />{/snippet}
{#snippet guide()}<AnalysisGuide />{/snippet}

<div
  class="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-0.5 border-y px-2 py-0.5"
  role="toolbar"
  aria-label="Both files">
  <Button variant="ghost" size="sm" disabled={!both} onclick={() => slots.swap()}
    ><ArrowUpDownIcon />Swap</Button>
  <label class={["flex items-center gap-2 text-sm", !both && "text-fg-muted"]}>
    <Switch bind:checked={spectrogramView.linked} disabled={!both} />
    Link zoom and scroll
  </label>
  {@render panel(A.compare.label, A.compare.icon, comparison, !compared)}
  <span class="ml-auto flex items-center gap-1">
    {@render panel("Queue", ListChecksIcon, queue, false, jobs.count)}
    {@render panel("Guide", CircleHelpIcon, guide)}
  </span>
</div>

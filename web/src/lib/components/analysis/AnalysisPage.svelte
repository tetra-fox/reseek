<script lang="ts">
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import { untrack } from "svelte";

  import { FileReport } from "#lib/analysis/report.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import { spectrogramView } from "#lib/analysis/spectrogram/view.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";

  import AnalysisGuide from "./AnalysisGuide.svelte";
  import AnalysisJobs from "./AnalysisJobs.svelte";
  import AnalysisSlot from "./AnalysisSlot.svelte";
  import CompareTable from "./CompareTable.svelte";
  import RecentResults from "./RecentResults.svelte";

  // a file on its own or two compared, one above the other, with the queue and the latest results
  // under them so the page is of use with nothing loaded
  const first = new FileReport(() => slots.a?.target ?? null, true);
  const second = new FileReport(() => slots.b?.target ?? null, true);

  const anything = $derived(!!slots.a || !!slots.b);
  const both = $derived(!!slots.a && !!slots.b);
  const heights = $derived(both ? { plot: 14, wave: 4 } : { plot: 22, wave: 5 });

  const a = $derived(first.report);
  const b = $derived(second.report);

  // the two viewers show the same stretch of time until they are unlinked
  $effect(() => {
    const [scopeA, scopeB] = [first.scope, second.scope];
    if (!scopeA || !scopeB || !spectrogramView.linked) return;
    untrack(() => scopeA.link(scopeB));
    return () => scopeA.unlink();
  });

  /** A file dropped beside a slot would replace the page with the file */
  const beside = (e: DragEvent) => e.preventDefault();
</script>

<svelte:window ondragover={beside} ondrop={beside} />

<div class="flex flex-col gap-3">
  <AnalysisSlot id="a" analyzed={first} slim={!!slots.b} {heights} />
  {#if both}
    <div class="flex flex-wrap items-center justify-center gap-x-4 gap-y-1">
      <Button variant="ghost" size="sm" onclick={() => slots.swap()}
        ><ArrowUpDownIcon />Swap</Button>
      <label class="flex items-center gap-2 text-sm">
        <Switch bind:checked={spectrogramView.linked} />
        Link zoom and scroll
      </label>
    </div>
  {/if}
  {#if anything || slots.sending.b}
    <AnalysisSlot id="b" analyzed={second} slim={!both} {heights} />
  {/if}
</div>

{#if a && b && !a.stale && !b.stale}
  <CompareTable {a} {b} />
{/if}

<AnalysisJobs />

<RecentResults />

{#if anything}
  <AnalysisGuide />
{/if}

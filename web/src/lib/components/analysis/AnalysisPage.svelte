<script lang="ts">
  import { untrack } from "svelte";

  import { FileReport } from "#lib/analysis/report.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { spectrogramView } from "#lib/analysis/spectrogram/view.svelte.js";

  import AnalysisSlot from "./AnalysisSlot.svelte";
  import AnalysisToolbar from "./AnalysisToolbar.svelte";

  // two rows edge to edge, a file or an empty placeholder in each, with the toolbar between them.
  // Each row takes half of the window, but never less than its slot asks for, so a plot stays
  // readable and the page scrolls when the window is too short for both
  const first = new FileReport(() => slots.a?.target ?? null, true);
  const second = new FileReport(() => slots.b?.target ?? null, true);

  const a = $derived(first.report);
  const b = $derived(second.report);
  const compared = $derived(a && b && !a.stale && !b.stale ? { a, b } : null);

  // the two viewers show the same stretch of time until they are unlinked. When one of them is
  // replaced, as by Analyze again, the new one takes the view of the one that stayed
  let pair: { a: Scope; b: Scope } | null = null;
  $effect(() => {
    const [scopeA, scopeB] = [first.scope, second.scope];
    if (!scopeA || !scopeB || !spectrogramView.linked) return;
    const lead = pair?.b === scopeB && pair.a !== scopeA ? scopeB : scopeA;
    pair = { a: scopeA, b: scopeB };
    untrack(() => lead.link(lead === scopeA ? scopeB : scopeA));
    return () => lead.unlink();
  });

  /** A file dropped beside a slot would replace the page with the file */
  const beside = (e: DragEvent) => e.preventDefault();
</script>

<svelte:window ondragover={beside} ondrop={beside} />

<div
  class="flex flex-col md:grid md:h-full md:grid-cols-[minmax(0,1fr)] md:grid-rows-[1fr_auto_1fr]">
  <AnalysisSlot id="a" analyzed={first} />
  <AnalysisToolbar {compared} />
  <AnalysisSlot id="b" analyzed={second} />
</div>

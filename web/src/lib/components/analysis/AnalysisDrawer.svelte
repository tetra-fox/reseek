<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";

  import type { Target } from "#lib/analysis/analysis.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { keepsKey, layerOpen, withModifier } from "#lib/keys.js";
  import { motionFast } from "#lib/motion.js";

  import AnalysisPanel from "./AnalysisPanel.svelte";

  // a local file's analysis in a drawer at the side, as the transfers view shows a download's,
  // with the file's own actions beside its name
  type Props = {
    target: Target;
    name: string;
    start: boolean;
    actions?: Snippet;
    onclose: () => void;
  };

  let { target, name, start, actions, onclose }: Props = $props();
</script>

<svelte:window
  onkeydown={(e) => {
    if (keepsKey(e) || withModifier(e) || layerOpen()) return;
    if (e.key === "Escape") onclose();
  }} />

<aside
  class="fixed top-(--top-h) right-0 bottom-(--bottom-h) z-30 flex w-[min(32rem,100vw)] flex-col border-l bg-surface shadow-raised"
  aria-label="Analysis"
  transition:fly|global={{ x: 32, duration: motionFast(), easing: cubicOut }}>
  <header class="flex items-center gap-2 border-b px-4 py-3">
    <h2 class="min-w-0 flex-1 truncate font-semibold" title={name}>{name}</h2>
    {@render actions?.()}
    <Button variant="ghost" size="icon-sm" aria-label="Close the analysis" onclick={onclose}
      ><XIcon /></Button>
  </header>
  <div class="flex-1 overflow-y-auto p-4">
    <AnalysisPanel {target} {start} />
  </div>
</aside>

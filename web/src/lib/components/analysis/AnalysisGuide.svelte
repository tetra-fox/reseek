<script lang="ts">
  import { DISCLAIMER } from "#lib/analysis/analysis.js";
  import Keys from "#lib/components/ui/Keys.svelte";
  import { ANALYSIS } from "#lib/ui/bindings.js";

  // what holds for every spectrogram on the page: the mouse and keyboard controls and the
  // disclaimer
  const MOUSE = [
    ["Click", "Place cursor"],
    ["Drag line", "Move cursor"],
    ["Drag", "Pan"],
    ["Scroll", "Zoom"],
    ["Shift+scroll", "Pan"]
  ];
</script>

<div class="flex flex-col gap-2">
  <dl class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-fg-muted" aria-label="Mouse and keyboard">
    {#each MOUSE as [keys, action] (keys)}
      <div class="flex items-center gap-1">
        <dt><kbd class="rounded-badge border px-1 font-sans">{keys}</kbd></dt>
        <dd>{action}</dd>
      </div>
    {/each}
    {#each Object.values(ANALYSIS) as b (b.label)}
      <div class="flex items-center gap-1">
        <dt><Keys binding={b} /></dt>
        <dd>{b.label}</dd>
      </div>
    {/each}
  </dl>
  <p
    class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
    {DISCLAIMER}
  </p>
</div>

<script lang="ts">
  import { DISCLAIMER } from "#lib/analysis/analysis.js";

  // what holds for every spectrogram on the page: the mouse and keyboard controls and the
  // disclaimer
  const CONTROLS = [
    ["Click", "Place cursor"],
    ["Drag line", "Move cursor"],
    ["Drag", "Pan"],
    ["Scroll", "Zoom"],
    ["Shift+scroll", "Pan"],
    ["+ / −", "Zoom"],
    ["← / →", "Previous or next window"],
    ["Shift+← / →", "Ten windows back or forward"],
    ["Home / End", "First or last window"],
    ["Space", "Play or pause"]
  ];
</script>

<div class="flex flex-col gap-2">
  <dl class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-fg-muted" aria-label="Mouse and keyboard">
    {#each CONTROLS as [keys, action] (keys)}
      <div class="flex items-center gap-1">
        <dt><kbd class="rounded-badge border px-1 font-sans">{keys}</kbd></dt>
        <dd>{action}</dd>
      </div>
    {/each}
  </dl>
  <p
    class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
    {DISCLAIMER}
  </p>
</div>

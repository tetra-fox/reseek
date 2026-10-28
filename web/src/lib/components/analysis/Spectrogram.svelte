<script lang="ts" module>
  /** The text shown with every spectrogram and every reading */
  export const DISCLAIMER =
    "This is a tool to help you judge, not proof. Content can stop short of the top of the graph for honest reasons: a test tone, speech, an old master or a vinyl rip all do. And a careful transcode can reach the top. Listen, compare, and decide for yourself.";
</script>

<script lang="ts">
  import ZoomInIcon from "@lucide/svelte/icons/zoom-in";
  import ZoomOutIcon from "@lucide/svelte/icons/zoom-out";
  import type { Component } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import {
    type Guide,
    heightOf,
    imageUrl,
    type Layout,
    type Target,
    windowAt
  } from "#lib/analysis/analysis.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { formatDuration } from "#lib/format.js";

  // the daemon's spectrogram, drawn without its guide lines, with the lines drawn over it from
  // the layout the daemon reports, so each line sits exactly where the image puts that frequency
  // and can be shown or hidden
  type Props = {
    target: Target;
    layout: Layout;
    /** where each analysed window starts, in seconds */
    starts: number[];
    /** the window picked to show its spectrum */
    picked: number | null;
    onpick: (index: number) => void;
  };

  let { target, layout, starts, picked, onpick }: Props = $props();

  /** each encoder line's own colour, so the legend tells them apart; all read on the dark plot */
  const ENCODER = ["#5ad2ff", "#7cffb2", "#c3a6ff", "#ffffff", "#ffb86b"];
  const colourOf = (g: Guide, i: number) =>
    g.kind === "file_limit" ? "#ff7878" : g.kind === "cutoff" ? "#ffd850" : ENCODER[i % 5];
  const lines = $derived(
    layout.guides.map((g, i) => ({
      ...g,
      key: `${g.kind}:${g.hz}`,
      colour: colourOf(g, layout.guides.filter((o) => o.kind === "encoder").indexOf(g)),
      y: layout.plotY + heightOf(layout, g.hz) * layout.plotHeight
    }))
  );
  const hidden = new SvelteSet<string>();
  const toggle = (key: string) => (hidden.has(key) ? hidden.delete(key) : hidden.add(key));

  const src = $derived(imageUrl(target, false));
  /** how many times its own width the image is shown at, no wider than its space at 1; a new
   *  image starts fitted */
  let zoom = $derived.by(() => {
    void [src, layout];
    return 1;
  });
  const ZOOMS = [1, 2, 4];

  let image = $state<HTMLImageElement | null>(null);
  /** whether the image failed to load, until another one is shown */
  let failed = $derived.by(() => {
    void [src, layout];
    return false;
  });

  /** Picks the window under a click on the plot */
  const pick = (e: MouseEvent) => {
    if (!image || !starts.length) return;
    const box = image.getBoundingClientRect();
    const x = ((e.clientX - box.left) / box.width) * layout.width - layout.plotX;
    const y = ((e.clientY - box.top) / box.height) * layout.height - layout.plotY;
    if (x < 0 || x >= layout.plotWidth || y < 0 || y >= layout.plotHeight) return;
    onpick(windowAt(layout, x / layout.plotWidth, starts.length));
  };
  /** the picked window's column, where its marker goes */
  const pickedX = $derived.by(() => {
    if (picked === null || !starts.length) return null;
    const column = Math.floor((picked * layout.plotWidth) / starts.length);
    return layout.plotX + column + 0.5;
  });
</script>

{#snippet zoomBy(label: string, Icon: Component, by: number)}
  {@const to = ZOOMS[ZOOMS.indexOf(zoom) + by]}
  <Tooltip text={label}>
    {#snippet child({ props })}
      <Button
        {...props}
        variant="ghost"
        size="icon-sm"
        aria-label={label}
        disabled={to === undefined}
        onclick={() => (zoom = to)}><Icon /></Button>
    {/snippet}
  </Tooltip>
{/snippet}

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-1" role="group" aria-label="Guide lines">
    {#each lines as line (line.key)}
      <button
        type="button"
        aria-pressed={!hidden.has(line.key)}
        class={[
          "flex items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-xs hover:bg-hover",
          hidden.has(line.key) && "opacity-40"
        ]}
        onclick={() => toggle(line.key)}>
        <span
          class="h-0.5 w-3 ring-1 ring-border"
          style:background={line.kind === "cutoff"
            ? `repeating-linear-gradient(90deg, ${line.colour} 0 3px, transparent 3px 5px)`
            : line.colour}></span>
        {line.label}
      </button>
    {/each}
    <span class="ml-auto flex gap-0.5">
      {@render zoomBy("Zoom out", ZoomOutIcon, -1)}
      {@render zoomBy("Zoom in", ZoomInIcon, 1)}
    </span>
  </div>
  <div class="overflow-x-auto rounded-control bg-[#101014]">
    {#if failed}
      <p class="text-white/70 p-4 text-sm">
        The spectrogram isn't available. Analyse the file again to draw it.
      </p>
    {:else}
      <!-- a click on the plot shows that window's spectrum; from the keyboard, the spectrum's
           previous and next buttons step through the windows -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div
        class={["relative", zoom === 1 && "max-w-full"]}
        style:width={`${zoom * layout.width}px`}
        onclick={pick}>
        <img
          bind:this={image}
          {src}
          alt="Spectrogram: frequency against time, louder content brighter"
          class="block h-auto w-full [image-rendering:pixelated]"
          width={layout.width}
          height={layout.height}
          onerror={() => (failed = true)} />
        <svg
          class="pointer-events-none absolute inset-0 size-full"
          viewBox={`0 0 ${layout.width} ${layout.height}`}
          preserveAspectRatio="none"
          aria-hidden="true">
          {#each lines as line (line.key)}
            {#if !hidden.has(line.key)}
              <line
                x1={layout.plotX}
                x2={layout.plotX + layout.plotWidth}
                y1={line.y}
                y2={line.y}
                stroke={line.colour}
                stroke-width="1"
                vector-effect="non-scaling-stroke"
                stroke-dasharray={line.kind === "cutoff" ? "4 3" : undefined}
                data-guide={line.label} />
            {/if}
          {/each}
          {#if pickedX !== null}
            <line
              x1={pickedX}
              x2={pickedX}
              y1={layout.plotY}
              y2={layout.plotY + layout.plotHeight}
              stroke="#ffffff"
              stroke-opacity="0.8"
              stroke-width="1"
              vector-effect="non-scaling-stroke" />
          {/if}
        </svg>
      </div>
    {/if}
  </div>
  <p class="text-xs text-fg-muted">
    The lines mark where common encoder settings cut off by default; other encoders and settings cut
    elsewhere.{layout.sampled
      ? ` The file is long, so ${starts.length} one-second windows spread over its ${formatDuration(starts.at(-1) ?? 0)} were analysed.`
      : ""}
  </p>
  <p
    class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
    {DISCLAIMER}
  </p>
</div>

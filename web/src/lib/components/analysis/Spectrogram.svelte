<script lang="ts" module>
  /** The text shown with every spectrogram and every reading */
  export const DISCLAIMER =
    "This is a tool to help you judge, not proof. Content can stop short of the top of the graph for honest reasons: a test tone, speech, an old master or a vinyl rip all do. And a careful transcode can reach the top. Listen, compare, and decide for yourself.";
</script>

<script lang="ts">
  import Maximize2Icon from "@lucide/svelte/icons/maximize-2";
  import ZoomInIcon from "@lucide/svelte/icons/zoom-in";
  import ZoomOutIcon from "@lucide/svelte/icons/zoom-out";
  import type { Component } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import {
    acrossAt,
    type Guide,
    heightOf,
    imageUrl,
    type Layout,
    SCALES,
    type Target,
    windowAt
  } from "#lib/analysis/analysis.js";
  import { spectrogramView } from "#lib/analysis/view.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { formatDuration } from "#lib/format.js";

  // the daemon's spectrogram across the file's whole length, drawn without its guide lines, with
  // the lines drawn over it from the layout the daemon reports, so each line sits exactly where
  // the image puts that frequency and can be shown or hidden
  type Props = {
    target: Target;
    layout: Layout;
    /** where each analysed window starts, in seconds */
    starts: number[];
    /** the window picked to show its spectrum */
    picked: number | null;
    onpick: (index: number) => void;
    /** opens the spectrogram across the whole screen; left out where it is shown that way */
    onlarge?: () => void;
  };

  let { target, layout, starts, picked, onpick, onlarge }: Props = $props();

  /** each encoder line's own colour, so the legend tells them apart; all read on the dark plot */
  const ENCODER = ["#5ad2ff", "#7cffb2", "#c3a6ff", "#ffffff", "#ffb86b"];
  const colourOf = (g: Guide, i: number) =>
    g.kind === "file_limit" ? "#ff7878" : g.kind === "cutoff" ? "#ffd850" : ENCODER[i % 5];
  const lines = $derived(
    layout.guides.map((g) => ({
      ...g,
      key: `${g.kind}:${g.hz}`,
      colour: colourOf(g, layout.guides.filter((o) => o.kind === "encoder").indexOf(g)),
      y: layout.plotY + heightOf(layout, g.hz) * layout.plotHeight
    }))
  );
  const hidden = new SvelteSet<string>();
  const toggle = (key: string) => (hidden.has(key) ? hidden.delete(key) : hidden.add(key));

  const src = $derived(imageUrl(target, false, layout.scale));
  /** how many times the space it is shown in the image is wide; a new image starts fitted */
  let zoom = $derived.by(() => {
    void [src, layout];
    return 1;
  });
  const ZOOMS = [1, 2, 4];
  /** the width the image is shown at, which past its own width keeps its columns sharp */
  let shownWidth = $state(0);

  let image = $state<HTMLImageElement | null>(null);
  /** whether the image failed to load, until another one is shown */
  let failed = $derived.by(() => {
    void [src, layout];
    return false;
  });

  /** Picks the window at the time under a click on the plot */
  const pick = (e: MouseEvent) => {
    if (!image || !starts.length) return;
    const box = image.getBoundingClientRect();
    const x = ((e.clientX - box.left) / box.width) * layout.width - layout.plotX;
    const y = ((e.clientY - box.top) / box.height) * layout.height - layout.plotY;
    if (x < 0 || x >= layout.plotWidth || y < 0 || y >= layout.plotHeight) return;
    onpick(windowAt(layout, x / layout.plotWidth, starts));
  };
  /** where the picked window's marker goes: the middle of its second */
  const pickedX = $derived(
    picked === null || starts[picked] === undefined
      ? null
      : layout.plotX + acrossAt(layout, starts[picked] + 0.5) * layout.plotWidth
  );
</script>

{#snippet tool(label: string, Icon: Component, run: () => void, disabled = false)}
  <Tooltip text={label}>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label={label} {disabled} onclick={run}
        ><Icon /></Button>
    {/snippet}
  </Tooltip>
{/snippet}

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-1">
    <div
      class="inline-flex rounded-control border p-0.5"
      role="radiogroup"
      aria-label="Frequency scale">
      {#each SCALES as s (s.id)}
        <button
          type="button"
          role="radio"
          aria-checked={layout.scale === s.id}
          class={[
            "rounded-badge px-2 py-0.5 text-xs",
            layout.scale === s.id ? "bg-surface-muted font-medium" : "text-fg-muted"
          ]}
          onclick={() => (spectrogramView.scale = s.id)}>{s.label}</button>
      {/each}
    </div>
    <span class="ml-auto flex gap-0.5">
      {@render tool(
        "Zoom out",
        ZoomOutIcon,
        () => (zoom = ZOOMS[ZOOMS.indexOf(zoom) - 1]),
        zoom === ZOOMS[0]
      )}
      {@render tool(
        "Zoom in",
        ZoomInIcon,
        () => (zoom = ZOOMS[ZOOMS.indexOf(zoom) + 1]),
        zoom === ZOOMS.at(-1)
      )}
      {#if onlarge}{@render tool("Open large", Maximize2Icon, onlarge)}{/if}
    </span>
  </div>
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
  </div>
  <div class="overflow-x-auto rounded-control bg-[#101014]">
    {#if failed}
      <p class="text-white/70 p-4 text-sm">
        The spectrogram isn't available. Analyse the file again to draw it.
      </p>
    {:else}
      <!-- a click on the plot shows that moment's spectrum; from the keyboard, the window strip
           in the reading picks a window -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div
        class="relative"
        style:width={`${zoom * 100}%`}
        bind:clientWidth={shownWidth}
        onclick={pick}>
        <img
          bind:this={image}
          {src}
          alt="Spectrogram: frequency against time over the whole file, louder content brighter"
          class={[
            "block h-auto w-full",
            shownWidth > layout.width && "[image-rendering:pixelated]"
          ]}
          width={layout.width}
          height={layout.height}
          style:aspect-ratio={`${layout.width} / ${layout.height}`}
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
              vector-effect="non-scaling-stroke"
              data-picked />
          {/if}
        </svg>
      </div>
    {/if}
  </div>
  <p class="text-xs text-fg-muted">
    The lines mark where common encoder settings cut off by default; other encoders and settings cut
    elsewhere.{layout.sampled
      ? ` The file is long, so ${starts.length} one-second samples spread over its ${formatDuration(layout.durationSeconds)} were analysed; the hatched stretches between them were not read.`
      : ""}
  </p>
  <p
    class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
    {DISCLAIMER}
  </p>
</div>

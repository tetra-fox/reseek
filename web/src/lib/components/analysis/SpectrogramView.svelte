<script lang="ts">
  import LocateFixedIcon from "@lucide/svelte/icons/locate-fixed";
  import LocateOffIcon from "@lucide/svelte/icons/locate-off";
  import Maximize2Icon from "@lucide/svelte/icons/maximize-2";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import ScanIcon from "@lucide/svelte/icons/scan";
  import ZoomInIcon from "@lucide/svelte/icons/zoom-in";
  import ZoomOutIcon from "@lucide/svelte/icons/zoom-out";
  import { type Component, untrack } from "svelte";

  import { DISCLAIMER, SCALES } from "#lib/analysis/analysis.js";
  import { guideColor, guideKey } from "#lib/analysis/spectrogram/draw.js";
  import { formatHz } from "#lib/analysis/spectrogram/frequency.js";
  import { ScaleMotion } from "#lib/analysis/spectrogram/scalemotion.svelte.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { formatTime, MIN_SPAN, showsAll } from "#lib/analysis/spectrogram/timeview.js";
  import { spectrogramView } from "#lib/analysis/spectrogram/view.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { playable, player, type Track } from "#lib/files/player.svelte.js";
  import { formatDuration } from "#lib/format.js";

  import Minimap from "./Minimap.svelte";
  import Timeline from "./Timeline.svelte";
  import WindowSpectrum from "./WindowSpectrum.svelte";

  // the spectrogram, the waveform and the spectrum at the cursor as one unit on one time axis,
  // with the frequency scale, zoom and playback above it, the guide lines' legend, a strip of the
  // whole file, what the cursor is on, and the disclaimer under it all. The large view puts the
  // spectrum beside the rest when there is room
  type Props = {
    scope: Scope;
    /** the file as the player plays it, null when it can't be played */
    track: Track | null;
    large?: boolean;
    /** opens the view across the whole screen; left out where it is shown that way */
    onlarge?: () => void;
  };

  let { scope, track, large = false, onlarge }: Props = $props();

  const layout = $derived(scope.layout);
  const scale = $derived(spectrogramView.scale ?? layout.scale);
  // the plots move to a new scale together; the choice itself is saved at the click
  const motion = new ScaleMotion(untrack(() => scale));
  $effect(() => motion.to(scale));
  $effect(() => () => motion.stop());
  const canPlay = $derived(!!track && playable(track.filename));
  const loaded = $derived(!!track && player.holds(track.key));
  const playing = $derived(loaded && player.playing);

  /** Plays the file from the cursor, or pauses and resumes it once it is in the player */
  const toggle = () => {
    if (!track || !canPlay) return;
    if (loaded) player.playing = !player.playing;
    else void player.play(track, [track], scope.cursor);
  };
  /** A reason the daemon gave, as a sentence */
  const sentence = (reason: string) =>
    `${reason.charAt(0).toUpperCase()}${reason.slice(1)}${/[.!?]$/.test(reason) ? "" : "."}`;

  /** The user put the cursor somewhere: playback moves there too */
  const seek = (t: number) => {
    if (loaded) player.seek(t);
  };

  const windows = $derived(scope.report.windows ?? []);
  const window = $derived(windows[scope.window]);
  const readout = $derived.by(() => {
    const at = formatTime(scope.cursor, true);
    if (!window) return at;
    const level =
      window.cutoffHz === null || window.cutoffHz === undefined
        ? "near silent"
        : `cutoff at ${formatHz(window.cutoffHz)}`;
    return `${at}, in the window at ${formatTime(window.t)}: ${window.rmsDbfs.toFixed(1)} dBFS, ${level}`;
  });

  let hint = $state(false);
  let hintTimer: ReturnType<typeof setTimeout> | undefined;
  const showHint = () => {
    hint = true;
    clearTimeout(hintTimer);
    hintTimer = setTimeout(() => (hint = false), 2500);
  };
  $effect(() => () => clearTimeout(hintTimer));

  const all = $derived(showsAll(scope.view, scope.duration));
  /** the mouse and keyboard controls, shown under the graph; the drawer zooms on Ctrl+scroll */
  const controls = $derived([
    ["Click", "Place the cursor"],
    ["Drag line", "Move the cursor"],
    ["Drag", "Pan"],
    [large ? "Scroll" : "Ctrl+scroll", "Zoom"],
    ["Shift+scroll", "Pan"],
    ["+ / −", "Zoom"],
    ["← / →", "Previous or next window"],
    ["Shift+← / →", "Ten windows back or forward"],
    ["Home / End", "First or last window"],
    ...(canPlay ? [["Space", "Play or pause"]] : [])
  ]);
  const deepest = $derived(scope.view.span <= Math.min(MIN_SPAN, scope.duration) + 1e-9);
</script>

{#snippet tool(label: string, Icon: Component, run: () => void, disabled = false)}
  <Tooltip text={label}>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label={label} {disabled} onclick={run}
        ><Icon /></Button>
    {/snippet}
  </Tooltip>
{/snippet}

<div class={["grid gap-3", large && "xl:grid-cols-[minmax(0,1fr)_22rem]"]}>
  <div class="flex min-w-0 flex-col gap-2">
    <div class="flex flex-wrap items-center gap-1">
      {#if canPlay}
        <Button variant="outline" size="sm" aria-pressed={playing} onclick={toggle}>
          {#if playing}<PauseIcon />Pause{:else}<PlayIcon />Play{/if}
        </Button>
        <Tooltip text={scope.following ? "Following the playhead" : "Not following the playhead"}>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label="Follow the playhead"
              aria-pressed={scope.following}
              onclick={() => scope.setFollowing(!scope.following)}>
              {#if scope.following}<LocateFixedIcon />{:else}<LocateOffIcon />{/if}
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
      <div
        class="inline-flex rounded-control border p-0.5"
        role="radiogroup"
        aria-label="Frequency scale">
        {#each SCALES as s (s.id)}
          <button
            type="button"
            role="radio"
            aria-checked={scale === s.id}
            class={[
              "rounded-badge px-2 py-0.5 text-xs",
              scale === s.id ? "bg-surface-muted font-medium" : "text-fg-muted"
            ]}
            onclick={() => (spectrogramView.scale = s.id)}>{s.label}</button>
        {/each}
      </div>
      <span class="ml-auto flex gap-0.5">
        {@render tool("Zoom out", ZoomOutIcon, () => scope.zoomAtCursor(0.5), all)}
        {@render tool("Zoom in", ZoomInIcon, () => scope.zoomAtCursor(2), deepest)}
        {@render tool("Show the whole file", ScanIcon, () => scope.showAll(), all)}
        {#if onlarge}{@render tool("Open large", Maximize2Icon, onlarge)}{/if}
      </span>
    </div>
    <div class="flex flex-wrap items-center gap-1" role="group" aria-label="Guide lines">
      {#each layout.guides as guide (guideKey(guide))}
        {@const key = guideKey(guide)}
        {@const color = guideColor(guide, layout.guides)}
        <button
          type="button"
          aria-pressed={!scope.hidden.has(key)}
          class={[
            "flex items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-xs hover:bg-hover",
            scope.hidden.has(key) && "opacity-40"
          ]}
          onclick={() =>
            scope.hidden.has(key) ? scope.hidden.delete(key) : scope.hidden.add(key)}>
          <span
            class="h-0.5 w-3 ring-1 ring-border"
            style:background={guide.kind === "cutoff"
              ? `repeating-linear-gradient(90deg, ${color} 0 3px, transparent 3px 5px)`
              : color}></span>
          {guide.label}
        </button>
      {/each}
    </div>
    {#if scope.pictures.error}
      <!-- the daemon's reason says what to do, such as analyzing the file again -->
      <p class="rounded-control bg-plot p-4 text-sm text-plot-fg">
        {sentence(scope.pictures.error)}
      </p>
    {:else}
      <Timeline
        {scope}
        {motion}
        {large}
        {readout}
        onseek={seek}
        ontoggle={toggle}
        onwheelhint={showHint} />
      <Minimap {scope} {motion} />
      {#if scope.pictures.lost}
        <p class="text-xs text-fg-muted" role="status">
          Part of the spectrogram could not be loaded and is shown at lower detail: {sentence(
            scope.pictures.lost
          )}
        </p>
      {/if}
      <!-- read out as the user moves the cursor, not as playback moves it every frame -->
      <p
        class="flex flex-wrap items-baseline gap-x-2 text-xs text-fg-muted"
        aria-live={playing ? "off" : "polite"}>
        <span class="font-medium text-fg tabular-nums">{readout}</span>
        {#if hint}<span>Hold Ctrl and scroll to zoom.</span>{/if}
      </p>
      {#if track && !canPlay}
        <p class="text-xs text-fg-muted">
          This browser cannot play this file. The cursor does not follow playback.
        </p>
      {/if}
    {/if}
  </div>
  <WindowSpectrum {scope} curve={motion.curve} />
  <div class={["flex flex-col gap-2", large && "xl:col-span-2"]}>
    <dl
      class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-fg-muted"
      aria-label="Mouse and keyboard">
      {#each controls as [keys, action] (keys)}
        <div class="flex items-center gap-1">
          <dt><kbd class="rounded-badge border px-1 font-sans">{keys}</kbd></dt>
          <dd>{action}</dd>
        </div>
      {/each}
    </dl>
    {#if layout.sampled}
      <p class="text-xs text-fg-muted">
        Only {scope.starts.length} one-second samples across the {formatDuration(
          layout.durationSeconds
        )} of this file were analyzed. The hatched stretches between them were not read.
      </p>
    {/if}
    <p
      class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
      {DISCLAIMER}
    </p>
  </div>
</div>

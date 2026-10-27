<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { timeLabel, timeTicks, valueLabel, valueTicks } from "#lib/dashboard/chart.js";
  import type { Point } from "#lib/dashboard/reports.js";
  import type { Series } from "#lib/dashboard/series.js";

  // transfer history as areas over time, a legend that shows and hides each series, and the
  // bucket under the pointer
  type Props = {
    data: Point[];
    series: Series[];
    /** the series drawn, by key */
    visible: SvelteSet<string>;
    label: string;
  };

  let { data, series, visible, label }: Props = $props();

  const HEIGHT = 200;
  const PAD = { top: 8, right: 8, bottom: 22, left: 64 };

  let width = $state(600);
  let hover = $state<number | null>(null);

  const shown = $derived(series.filter((s) => visible.has(s.key)));
  const units = $derived(new Set(shown.map((s) => s.unit)));
  const format = $derived(valueLabel(units.size === 1 ? [...units][0] : null));
  const min = $derived(data[0]?.timestamp ?? 0);
  const max = $derived(data.at(-1)?.timestamp ?? 0);
  const top = $derived(Math.max(0, ...data.flatMap((p) => shown.map((s) => p[s.key]))));
  const yTicks = $derived(valueTicks(top));
  const yMax = $derived(yTicks.at(-1) || 1);
  const xTicks = $derived(timeTicks(min, max));
  const xLabel = $derived(timeLabel(min, max));

  const x = (t: number) =>
    PAD.left + (max > min ? ((t - min) / (max - min)) * (width - PAD.left - PAD.right) : 0);
  const y = (v: number) => PAD.top + (1 - v / yMax) * (HEIGHT - PAD.top - PAD.bottom);
  const area = (s: Series) => {
    if (!data.length) return "";
    const line = data.map((p) => `${x(p.timestamp)},${y(p[s.key])}`).join(" L");
    return `M${line} L${x(max)},${y(0)} L${x(min)},${y(0)} Z`;
  };
  const edge = (s: Series) => `M${data.map((p) => `${x(p.timestamp)},${y(p[s.key])}`).join(" L")}`;

  const point = (e: PointerEvent & { currentTarget: SVGSVGElement }) => {
    if (!data.length) return;
    const box = e.currentTarget.getBoundingClientRect();
    const at =
      min + ((e.clientX - box.left - PAD.left) / (width - PAD.left - PAD.right)) * (max - min);
    let best = 0;
    for (let i = 1; i < data.length; i++) {
      if (Math.abs(data[i].timestamp - at) < Math.abs(data[best].timestamp - at)) best = i;
    }
    hover = best;
  };
  const toggle = (key: string) => (visible.has(key) ? visible.delete(key) : visible.add(key));
</script>

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap gap-1" role="group" aria-label={`${label} series`}>
    {#each series as s (s.key)}
      <button
        type="button"
        aria-pressed={visible.has(s.key)}
        class={[
          "flex items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-xs hover:bg-hover",
          !visible.has(s.key) && "opacity-35"
        ]}
        onclick={() => toggle(s.key)}>
        <span class="rounded-sm size-2.5" style:background-color={s.color}></span>{s.name}
      </button>
    {/each}
  </div>
  <div class="relative" bind:clientWidth={width}>
    <svg
      role="img"
      aria-label={label}
      {width}
      height={HEIGHT}
      class="block"
      onpointermove={point}
      onpointerleave={() => (hover = null)}>
      {#each yTicks as t (t)}
        <line
          x1={PAD.left}
          x2={width - PAD.right}
          y1={y(t)}
          y2={y(t)}
          class="stroke-border"
          stroke-dasharray="3 3" />
        <text
          x={PAD.left - 6}
          y={y(t)}
          dy="0.32em"
          text-anchor="end"
          class="fill-fg-muted text-[10px]">{format(t)}</text>
      {/each}
      {#each xTicks as t (t)}
        <text x={x(t)} y={HEIGHT - 6} text-anchor="middle" class="fill-fg-muted text-[10px]"
          >{xLabel(t)}</text>
      {/each}
      {#each shown as s (s.key)}
        <path d={area(s)} fill={s.color} fill-opacity="0.15" />
        <path d={edge(s)} fill="none" stroke={s.color} stroke-width="1.5" />
      {/each}
      {#if hover !== null && data[hover]}
        <line
          x1={x(data[hover].timestamp)}
          x2={x(data[hover].timestamp)}
          y1={PAD.top}
          y2={HEIGHT - PAD.bottom}
          class="stroke-fg-subtle" />
      {/if}
    </svg>
    {#if hover !== null && data[hover] && shown.length}
      {@const p = data[hover]}
      <div
        class="pointer-events-none absolute top-2 rounded-control border bg-overlay px-2 py-1 text-xs shadow-raised"
        style:left={`${Math.min(x(p.timestamp) + 8, width - 180)}px`}>
        <p class="font-medium">{xLabel(p.timestamp)}</p>
        {#each shown as s (s.key)}
          <p class="flex items-center gap-1.5">
            <span class="rounded-sm size-2" style:background-color={s.color}></span>{s.name}:
            {valueLabel(s.unit)(p[s.key])}
          </p>
        {/each}
      </div>
    {/if}
  </div>
</div>

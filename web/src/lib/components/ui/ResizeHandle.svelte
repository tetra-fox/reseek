<script lang="ts">
  import type { Snippet } from "svelte";

  import { cn } from "#lib/ui/cn.js";
  import { withModifier } from "#lib/ui/keys.js";

  type Props = {
    /** the height or width it sets, in pixels */
    value: number;
    min: number;
    max?: number;
    /** the edge of the resized element it sits on; dragging away from the element grows it */
    edge: "top" | "bottom" | "left";
    label: string;
    /** a drag or a key press has ended on a new size, for keeping it */
    onchange: (value: number) => void;
    class?: string;
    children?: Snippet;
  };

  let {
    value = $bindable(),
    min,
    max = Infinity,
    edge,
    label,
    onchange,
    class: className,
    children
  }: Props = $props();

  let dragging: { at: number; value: number } | null = null;
  const sideways = $derived(edge === "left");
  /** the keys that move the handle back and forward along its axis */
  const keys = $derived(sideways ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"]);
  const clamp = (v: number) => Math.round(Math.min(max, Math.max(min, v)));
  /** where a pointer is along the axis the handle moves on */
  const along = (e: PointerEvent) => (sideways ? e.clientX : e.clientY);
  /** how far a move of `by` pixels down or to the right grows the element */
  const grows = (by: number) => (edge === "bottom" ? by : -by);
</script>

<div
  role="slider"
  tabindex="0"
  aria-orientation={sideways ? "horizontal" : "vertical"}
  aria-label={label}
  aria-valuemin={min}
  aria-valuemax={Number.isFinite(max) ? max : undefined}
  aria-valuenow={value}
  title="Drag or use the arrow keys to resize"
  class={cn(sideways ? "cursor-col-resize" : "cursor-row-resize", className)}
  onpointerdown={(e) => {
    dragging = { at: along(e), value };
    e.currentTarget.setPointerCapture(e.pointerId);
  }}
  onpointermove={(e) => {
    if (dragging) value = clamp(dragging.value + grows(along(e) - dragging.at));
  }}
  onpointerup={() => {
    if (dragging) onchange(value);
    dragging = null;
  }}
  onkeydown={(e) => {
    if (withModifier(e) || !keys.includes(e.key)) return;
    e.preventDefault();
    value = clamp(value + grows(e.key === keys[1] ? 20 : -20));
    onchange(value);
  }}>
  {@render children?.()}
</div>

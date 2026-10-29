<script lang="ts">
  import type { Snippet } from "svelte";

  import { cn } from "#lib/ui/cn.js";
  import { withModifier } from "#lib/ui/keys.js";

  type Props = {
    /** the height it sets, in pixels */
    value: number;
    min: number;
    max?: number;
    /** the edge of the resized element it sits on; dragging away from the element grows it */
    edge: "top" | "bottom";
    label: string;
    /** a drag or a key press has ended on a new height, for keeping it */
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

  let dragging: { y: number; value: number } | null = null;
  const clamp = (v: number) => Math.round(Math.min(max, Math.max(min, v)));
  /** how far a move of `dy` pixels down grows the element */
  const grows = (dy: number) => (edge === "top" ? -dy : dy);
</script>

<div
  role="slider"
  tabindex="0"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuemin={min}
  aria-valuemax={Number.isFinite(max) ? max : undefined}
  aria-valuenow={value}
  title="Drag or use the arrow keys to resize"
  class={cn("cursor-row-resize", className)}
  onpointerdown={(e) => {
    dragging = { y: e.clientY, value };
    e.currentTarget.setPointerCapture(e.pointerId);
  }}
  onpointermove={(e) => {
    if (dragging) value = clamp(dragging.value + grows(e.clientY - dragging.y));
  }}
  onpointerup={() => {
    if (dragging) onchange(value);
    dragging = null;
  }}
  onkeydown={(e) => {
    if (withModifier(e) || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    value = clamp(value + grows(e.key === "ArrowDown" ? 20 : -20));
    onchange(value);
  }}>
  {@render children?.()}
</div>

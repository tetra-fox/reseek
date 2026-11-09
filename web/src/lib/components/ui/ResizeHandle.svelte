<script lang="ts">
  import type { Snippet } from "svelte";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { cn } from "#lib/ui/cn.js";
  import { withModifier } from "#lib/ui/keys.js";

  type Props = {
    /** the height or width it sets, in pixels */
    value: number;
    min: number;
    max?: number;
    /** the size a double click goes back to */
    initial: number;
    /** the edge of the resized element it sits on; dragging away from the element grows it */
    edge: "top" | "bottom" | "left" | "right";
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
    initial,
    edge,
    label,
    onchange,
    class: className,
    children
  }: Props = $props();

  /** how far a press of an arrow key moves the edge, in pixels */
  const STEP = 20;

  let dragging = $state.raw<{ at: number; value: number } | null>(null);
  const sideways = $derived(edge === "left" || edge === "right");
  /** the keys that move the handle back and forward along its axis */
  const keys = $derived(sideways ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"]);
  const clamp = (v: number) => Math.round(Math.min(max, Math.max(min, v)));
  /**
   * the size in effect: a window made smaller lowers `max` under a size kept from before, which
   * the element shows no larger than `max`
   */
  const size = $derived(clamp(value));
  /** where a pointer is along the axis the handle moves on */
  const along = (e: PointerEvent) => (sideways ? e.clientX : e.clientY);
  /** how far a move of `by` pixels down or to the right grows the element */
  const grows = (by: number) => (edge === "bottom" || edge === "right" ? by : -by);
  const set = (next: number) => {
    value = clamp(next);
    onchange(value);
  };
</script>

<!-- a separator between two parts of a screen that the keys can move, as a splitter is. Its line shows
     once the pointer rests on it, and at once for the keys and during a drag. A press takes focus
     by hand, as keeping the page from selecting text under a drag also keeps it from focusing. The
     pointer handlers listen in the capture phase so the tooltip's own on the same events still run -->
<Tooltip
  text="Drag or use the arrow keys to resize, double-click to reset"
  side={sideways ? "right" : "top"}>
  {#snippet child({ props })}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      {...props}
      role="separator"
      tabindex="0"
      aria-orientation={sideways ? "vertical" : "horizontal"}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={Number.isFinite(max) ? max : undefined}
      aria-valuenow={size}
      data-dragging={dragging ? "" : undefined}
      class={cn(
        "touch-none outline-none after:absolute after:bg-accent after:opacity-0 after:transition-opacity after:duration-(--motion-fast) hover:after:opacity-100 hover:after:delay-150 focus-visible:after:opacity-100 data-dragging:after:opacity-100 data-dragging:after:delay-0",
        sideways
          ? "cursor-col-resize after:inset-y-0 after:left-1/2 after:w-0.5 after:-translate-x-1/2"
          : "cursor-row-resize after:inset-x-0 after:top-1/2 after:h-0.5 after:-translate-y-1/2",
        className
      )}
      onpointerdowncapture={(e) => {
        e.preventDefault();
        e.currentTarget.focus();
        dragging = { at: along(e), value: size };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onpointermovecapture={(e) => {
        if (dragging) value = clamp(dragging.value + grows(along(e) - dragging.at));
      }}
      onlostpointercapture={() => {
        if (dragging && value !== dragging.value) onchange(value);
        dragging = null;
      }}
      ondblclick={() => set(initial)}
      onkeydown={(e) => {
        if (withModifier(e)) return;
        if (e.key === "Home") set(min);
        else if (e.key === "End" && Number.isFinite(max)) set(max);
        else if (keys.includes(e.key)) set(size + grows(e.key === keys[1] ? STEP : -STEP));
        else return;
        e.preventDefault();
      }}>
      {@render children?.()}
    </div>
  {/snippet}
</Tooltip>

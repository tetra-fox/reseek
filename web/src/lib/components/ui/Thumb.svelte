<script lang="ts" module>
  type Place = { x: number; y: number; width: number; height: number };

  /**
   * Whether the thumb slides to the chosen item or is put there at once. It slides only when a
   * new choice is on the same line: not as it first shows, not to another line, where it would
   * cut across, and not when the track resizes, where it would trail behind
   */
  export const slides = (from: Place | undefined, to: Place, chosen: boolean) =>
    chosen && from !== undefined && from.y === to.y;
</script>

<script lang="ts">
  import type { ClassValue } from "svelte/elements";

  // the highlight behind the chosen item of a radio group or tab list, as the track's first child.
  // The track is `relative` and its items `relative z-1`, so they draw over it
  type Props = {
    /** the chosen item's value, which bits-ui writes on the item as `data-value` */
    value: string;
    class?: ClassValue;
  };

  let { value, class: className }: Props = $props();

  let thumb: HTMLSpanElement;
  let shown = $state<{ place: Place; sliding: boolean }>();
  // the last place, kept out of state so the effects that write it don't also depend on it
  let last: Place | undefined;

  const measure = (chosen: boolean) => {
    const item = thumb.parentElement!.querySelector<HTMLElement>(
      `:scope > [data-value="${CSS.escape(value)}"]`
    );
    // a value that matches no item, as when a setting holds one the choices don't offer
    if (!item) {
      shown = last = undefined;
      return;
    }
    const place = {
      x: item.offsetLeft,
      y: item.offsetTop,
      width: item.offsetWidth,
      height: item.offsetHeight
    };
    shown = { place, sliding: slides(last, place, chosen) };
    last = place;
  };

  // reads `value`, so it runs again on each new choice
  $effect(() => measure(true));

  // text size, wrapping and a resized panel all change the track's size
  $effect(() => {
    const observer = new ResizeObserver(() => measure(false));
    observer.observe(thumb.parentElement!);
    return () => observer.disconnect();
  });
</script>

<span
  bind:this={thumb}
  aria-hidden="true"
  hidden={!shown}
  style:translate={shown && `${shown.place.x}px ${shown.place.y}px`}
  style:width={shown && `${shown.place.width}px`}
  style:height={shown && `${shown.place.height}px`}
  class={[
    "pointer-events-none absolute top-0 left-0",
    shown?.sliding &&
      "transition-[translate,width] duration-(--motion-fast) ease-(--ease-out-soft)",
    className
  ]}></span>

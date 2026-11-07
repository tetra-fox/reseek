<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CopyGlyph from "@lucide/svelte/icons/copy";
  import type { ClassValue } from "svelte/elements";

  type Props = {
    /** shows the check in place of the copy glyph */
    copied: boolean;
    /** classes for the check, such as its color */
    class?: ClassValue;
  };

  let { copied, class: checkClass }: Props = $props();

  const glyph = "origin-center transition-[opacity,scale] duration-(--motion-fast) ease-out-soft";
</script>

<!-- both glyphs in one svg, so a button sizes and pads it as its own icon; the one shown settles in
from slightly smaller as the other fades -->
<svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">
  <g class={[glyph, copied && "scale-85 opacity-0"]}><CopyGlyph class="size-full" /></g>
  <g class={[glyph, !copied && "scale-85 opacity-0", checkClass]}>
    <CheckIcon class="size-full" />
  </g>
</svg>

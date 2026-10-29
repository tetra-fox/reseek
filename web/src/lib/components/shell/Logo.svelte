<script lang="ts">
  import AudioWaveformIcon from "@lucide/svelte/icons/audio-waveform";

  import { cn } from "#lib/ui/cn.js";

  let { class: className }: { class?: string } = $props();
</script>

<span
  class={cn(
    "flex size-control shrink-0 items-center justify-center rounded-control bg-accent text-accent-fg shadow-xs",
    className
  )}>
  <AudioWaveformIcon class="size-1/2" />
</span>

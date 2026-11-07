<script lang="ts">
  import { Progress } from "bits-ui";

  import { cn } from "#lib/ui/cn.js";

  type Props = {
    /** how far the task is, 0 to 100; null while it has no progress yet, as a queued one */
    value: number | null;
    /** what is in progress, for a screen reader */
    label: string;
    /** the track's size and frame */
    class?: string;
  };

  let { value, label, class: className }: Props = $props();

  const percent = $derived(value === null ? null : Math.round(value));
</script>

<!-- the fill moves by translate rather than width, so bars updating in a long list cause no layout -->
<Progress.Root
  value={percent}
  max={100}
  aria-label={label}
  class={cn("h-1.5 overflow-hidden rounded-full bg-surface-muted", className)}>
  <div
    class="size-full rounded-full bg-accent transition-[translate] duration-(--motion-slow) ease-(--ease-out-soft)"
    style:translate={`${(percent ?? 0) - 100}% 0`}>
  </div>
</Progress.Root>

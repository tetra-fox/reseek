<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import MinusIcon from "@lucide/svelte/icons/minus";
  import { Checkbox } from "bits-ui";

  import { cn } from "#lib/ui/cn.js";

  type Props = {
    "checked": boolean;
    /** some but not all of what it stands for is checked */
    "indeterminate"?: boolean;
    "onCheckedChange"?: (checked: boolean) => void;
    "aria-label"?: string;
    "disabled"?: boolean;
    "class"?: string;
    /**
     * A list row's tick, in place of `onCheckedChange`: a click ticks the row, with Shift the rows
     * from the last one ticked, and Space ticks it alone
     */
    "ontick"?: (range: boolean) => void;
  };

  let {
    checked,
    indeterminate = false,
    onCheckedChange,
    ontick,
    class: className,
    ...rest
  }: Props = $props();
</script>

<!-- controlled: the caller decides what checking it does and passes the result back. The box is
     small to look at, so its ::after takes clicks a little past it -->
<Checkbox.Root
  {checked}
  {indeterminate}
  onCheckedChange={ontick ? (on) => on !== checked && ontick(false) : onCheckedChange}
  onclick={ontick &&
    ((e: MouseEvent) => {
      // the click is handled here, for its Shift, and the box's own toggle skipped; Space still
      // toggles it through the change
      e.preventDefault();
      ontick(e.shiftKey);
    })}
  class={cn(
    "relative inline-flex size-4 shrink-0 items-center justify-center rounded-[0.25rem] border border-input bg-surface align-middle text-accent-fg shadow-xs transition-colors duration-(--motion-fast) after:absolute after:-inset-x-1 after:-inset-y-1.5 disabled:opacity-50 data-[state=checked]:border-accent data-[state=checked]:bg-accent data-[state=indeterminate]:border-accent data-[state=indeterminate]:bg-accent",
    className
  )}
  {...rest}>
  {#snippet children({ checked: on, indeterminate: some })}
    {#if some}<MinusIcon class="size-3" />{:else if on}<CheckIcon class="size-3" />{/if}
  {/snippet}
</Checkbox.Root>

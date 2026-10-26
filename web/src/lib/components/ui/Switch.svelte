<script lang="ts">
  import { Switch } from "bits-ui";

  import { cn } from "#lib/utils.js";

  type Props = {
    "checked"?: boolean;
    "onCheckedChange"?: (checked: boolean) => void;
    "id"?: string;
    "aria-label"?: string;
    "aria-describedby"?: string;
    "disabled"?: boolean;
    "class"?: string;
  };

  let { checked = $bindable(false), onCheckedChange, class: className, ...rest }: Props = $props();
</script>

<Switch.Root
  bind:checked
  {onCheckedChange}
  class={cn(
    "inline-flex h-5 w-9 shrink-0 items-center rounded-full p-0.5 transition-colors duration-(--motion-fast) disabled:opacity-50",
    checked ? "bg-accent" : "bg-input",
    className
  )}
  {...rest}>
  <Switch.Thumb
    class={[
      "pointer-events-none block size-4 rounded-full shadow-xs transition-transform duration-(--motion-fast) ease-(--ease-out-soft)",
      checked ? "translate-x-4 bg-accent-fg" : "bg-fg-muted"
    ]} />
</Switch.Root>

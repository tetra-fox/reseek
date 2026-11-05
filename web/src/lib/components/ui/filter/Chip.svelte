<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import EyeIcon from "@lucide/svelte/icons/eye";
  import EyeOffIcon from "@lucide/svelte/icons/eye-off";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import type { HTMLButtonAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  import { FILTERING } from "./tint";

  type Props = HTMLButtonAttributes & {
    pressed: boolean;
    /**
     * pressing it off hides what it names, as a log level, so it filters while off; otherwise
     * pressing it on keeps only what it names, and it filters while on
     */
    hides?: boolean;
    /** how many items it stands for */
    count?: number;
  };

  let { pressed, hides = false, count, class: className, children, ...rest }: Props = $props();

  // a chip that narrows the list is tinted and marked by its icon, so it reads apart from the
  // others without its color; every chip has an icon, so pressing one moves nothing beside it
  const filtering = $derived(hides ? !pressed : pressed);
  const Icon = $derived(hides ? (pressed ? EyeIcon : EyeOffIcon) : pressed ? CheckIcon : PlusIcon);
</script>

<button
  type="button"
  aria-pressed={pressed}
  class={cn(
    "inline-flex h-control-sm shrink-0 items-center gap-1.5 rounded-full border pr-3 pl-2 text-sm font-medium whitespace-nowrap transition-colors duration-(--motion-fast) disabled:opacity-50",
    filtering
      ? [FILTERING, "not-disabled:hover:bg-accent-soft/70"]
      : "border-input bg-surface text-fg not-disabled:hover:bg-hover",
    className
  )}
  data-label
  {...rest}>
  <Icon class={["size-3.5 shrink-0", !filtering && "text-fg-subtle"]} />
  <span class={[hides && !pressed && "line-through"]}>{@render children?.()}</span>
  {#if count !== undefined}
    <span class={["tabular-nums", !filtering && "text-fg-muted"]}>{count.toLocaleString()}</span>
  {/if}
</button>

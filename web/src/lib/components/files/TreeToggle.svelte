<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { type Expansion, withSiblings } from "#lib/tree/expansion.svelte.js";
  import { TOGGLE_REM } from "#lib/tree/indent.js";
  import { cn } from "#lib/ui/cn.js";

  type Props = {
    expansion: Expansion;
    /** what it opens and closes */
    key: string;
    /** the keys beside it, which Alt-click opens or closes with it */
    siblings?: string[];
    /** names what opens, for the button's label */
    name: string;
    /** after it opened or closed, with the keys that changed */
    ontoggle?: (open: boolean, keys: string[]) => void;
    class?: string;
  };

  let { expansion, key, siblings = [], name, ontoggle, class: className }: Props = $props();

  const open = $derived(expansion.isOpen(key));
</script>

<Tooltip text="Alt-click to open or close the sibling folders as well">
  {#snippet child({ props })}
    <button
      {...props}
      type="button"
      class={cn(
        "flex shrink-0 items-center justify-center rounded-badge text-fg-muted hover:bg-hover hover:text-fg",
        className
      )}
      style:width={`${TOGGLE_REM}rem`}
      style:height={`${TOGGLE_REM}rem`}
      aria-expanded={open}
      aria-label={`${open ? "Close" : "Open"} ${name}`}
      onclick={(e) => {
        const all = withSiblings(e);
        expansion.toggle(key, siblings, all);
        ontoggle?.(!open, all ? [key, ...siblings.filter((s) => s !== key)] : [key]);
      }}>
      <ChevronRightIcon
        class={["size-4 transition-transform duration-(--motion-fast)", open && "rotate-90"]} />
    </button>
  {/snippet}
</Tooltip>

<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  import { type Expansion, withSiblings } from "#lib/tree/expansion.svelte.js";
  import { cn } from "#lib/utils.js";

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

<button
  type="button"
  class={cn(
    "flex size-5 shrink-0 items-center justify-center rounded-badge text-fg-muted hover:bg-hover hover:text-fg",
    className
  )}
  aria-expanded={open}
  aria-label={`${open ? "Close" : "Open"} ${name}`}
  title="Alt-click to open or close the ones beside it too"
  onclick={(e) => {
    const all = withSiblings(e);
    expansion.toggle(key, siblings, all);
    ontoggle?.(!open, all ? [key, ...siblings.filter((s) => s !== key)] : [key]);
  }}>
  <ChevronRightIcon
    class={["size-4 transition-transform duration-(--motion-fast)", open && "rotate-90"]} />
</button>

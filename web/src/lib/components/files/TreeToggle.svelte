<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { alsoToggled, type Expansion } from "#lib/tree/expansion.svelte.js";
  import { TOGGLE_REM } from "#lib/tree/indent.js";
  import { cn } from "#lib/ui/cn.js";
  import { mac } from "#lib/ui/keys.js";

  type Props = {
    expansion: Expansion;
    /** what it opens and closes */
    key: string;
    /** the keys beside it, which an Alt-click opens or closes with it */
    siblings?: string[];
    /** the keys of the folders inside it, which a Ctrl-click opens or closes with it */
    inside?: () => string[];
    /** names what opens, for the button's label */
    name: string;
    /** after it opened or closed, with the keys that changed */
    ontoggle?: (open: boolean, keys: string[]) => void;
    class?: string;
  };

  let {
    expansion,
    key,
    siblings = [],
    inside = () => [],
    name,
    ontoggle,
    class: className
  }: Props = $props();

  const open = $derived(expansion.isOpen(key));
</script>

<Tooltip
  text={`${mac ? "Option" : "Alt"}-click for the folders beside it too, ${mac ? "Cmd" : "Ctrl"}-click for every folder inside`}>
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
        const also = alsoToggled(e, key, siblings, inside);
        expansion.toggle(key, also);
        ontoggle?.(!open, [key, ...also]);
      }}>
      <ChevronRightIcon
        class={["size-4 transition-transform duration-(--motion-fast)", open && "rotate-90"]} />
    </button>
  {/snippet}
</Tooltip>

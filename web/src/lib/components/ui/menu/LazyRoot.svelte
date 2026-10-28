<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import type { Snippet } from "svelte";

  import { setAnchor } from "./anchor";

  // a menu that is only built once it is first opened: a long list's rows each have one, and
  // building a menu for every row as it scrolls into view costs more than the rest of the row.
  // The trigger is the caller's own button, which spreads `props`
  type Props = {
    trigger: Snippet<[{ props: Record<string, unknown> }]>;
    /** the menu's content, a Menu.Content */
    children: Snippet;
  };

  let { trigger, children }: Props = $props();

  let open = $state(false);
  /** whether it was ever opened, which keeps it built so its closing can play out */
  let built = $state(false);
  let anchor = $state<HTMLElement | null>(null);
  setAnchor(() => anchor);

  const opener = $derived({
    "aria-haspopup": "menu",
    "aria-expanded": open,
    "onclick": (e: MouseEvent) => {
      anchor = e.currentTarget as HTMLElement;
      built = true;
      open = !open;
    }
  });
</script>

{@render trigger({ props: opener })}
{#if built}
  <DropdownMenu.Root bind:open>
    {@render children()}
  </DropdownMenu.Root>
{/if}

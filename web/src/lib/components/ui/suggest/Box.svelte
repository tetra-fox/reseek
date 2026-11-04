<script lang="ts">
  import { Combobox } from "bits-ui";
  import type { ComponentProps, Snippet } from "svelte";

  import Input from "#lib/components/ui/Input.svelte";

  import { type Highlight, shareHighlight } from "./highlight";

  type Props = Omit<ComponentProps<typeof Input>, "value" | "ref" | "children" | "onkeydown"> & {
    /** the text in the box */
    value: string;
    /** the box's element */
    ref?: HTMLInputElement | null;
    /** the dropdown is wanted; it also takes text in the box and something to list */
    open?: boolean;
    /**
     * the entry that comes first, which has the highlight until the keys move it and which Enter
     * takes then, null when Enter has nothing to take
     */
    first: string | null;
    /** there is something to list */
    listed?: boolean;
    /** holds the dropdown open while an entry's work runs */
    hold?: boolean;
    /** what Enter waits for before it takes the first entry, which the answer can change */
    settle?: () => Promise<unknown> | undefined;
    /** an entry was picked, by the keys or the pointer */
    onchoose: (value: string) => void;
    /** Escape or a click elsewhere closed the dropdown */
    ondismiss?: () => void;
    /** the entries, as `Item` and `Group` */
    children: Snippet;
  };

  let {
    value = $bindable(),
    ref = $bindable(null),
    open = $bindable(false),
    first,
    listed = true,
    hold = false,
    settle,
    onchoose,
    ondismiss,
    children,
    ...input
  }: Props = $props();

  /** what the dropdown reports as picked, emptied at once so an entry can be picked again */
  let picked = $state("");
  const highlight = $state<Highlight>({ lit: null });
  shareHighlight(highlight);

  // no name has spaces around it, so what is around the text is no part of it
  const text = $derived(value.trim());
  const shown = $derived((open || hold) && !!text && listed);

  const choose = (entry: string) => {
    picked = "";
    open = false;
    highlight.lit = null;
    onchoose(entry);
  };

  /**
   * Enter takes the entry the keys moved to, else the first once `settle` has answered; text
   * typed while it waits is a new search, which the Enter was not for
   */
  const enter = async () => {
    if (shown && highlight.lit !== null && highlight.lit !== first) return choose(highlight.lit);
    const asked = text;
    const answer = settle?.();
    if (answer) await answer;
    if (first !== null && text === asked) choose(first);
  };

  // an empty box leaves Enter to the form the box is in
  const keydown = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || e.isComposing || !text) return;
    e.preventDefault();
    void enter();
  };
</script>

<Combobox.Root
  type="single"
  bind:open={() => shown, (on) => (open = on)}
  bind:value={picked}
  onValueChange={choose}>
  <Combobox.Input oninput={() => (open = true)} onclick={() => (open = true)} onkeydown={keydown}>
    {#snippet child({ props })}
      <Input {...props} {...input} bind:ref bind:value />
    {/snippet}
  </Combobox.Input>
  <Combobox.Portal>
    <Combobox.Content
      sideOffset={4}
      onEscapeKeydown={ondismiss}
      onInteractOutside={ondismiss}
      class="z-50 flex max-h-(--bits-combobox-content-available-height) w-[max(var(--bits-combobox-anchor-width),20rem)] max-w-[calc(100vw-1rem)] origin-(--bits-combobox-content-transform-origin) flex-col rounded-control border bg-overlay p-1 text-fg shadow-raised data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <Combobox.Viewport class="min-h-0">
        {@render children()}
      </Combobox.Viewport>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>

<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import type { Component } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  type Props = HTMLInputAttributes & {
    /** the element, for callers that focus it */
    ref?: HTMLInputElement | null;
    /** an icon inside the box, before the text */
    icon?: Component;
    /** a search box's wrapper, which takes the box's width and place in the layout */
    wrapperClass?: string;
    /** after the clear button emptied the box */
    onclear?: () => void;
  };

  let {
    value = $bindable(),
    ref = $bindable(null),
    type,
    icon: Icon,
    wrapperClass,
    onclear,
    class: className,
    ...rest
  }: Props = $props();

  // a search box gets a button that empties it, and keeps room for it only while it shows, so a
  // placeholder has the whole box; the browser's own is hidden in app.css
  const clearable = $derived(type === "search");

  const clear = () => {
    value = "";
    ref?.focus();
    onclear?.();
  };
</script>

{#snippet field()}
  <!-- text-base below md keeps iOS from zooming in on focus -->
  <input
    bind:this={ref}
    class={cn(
      "h-control w-full min-w-0 rounded-control border border-input bg-surface px-3 text-base text-fg shadow-xs transition-colors duration-(--motion-fast) placeholder:text-fg-muted placeholder-shown:text-ellipsis hover:border-fg-subtle/50 focus-visible:border-accent disabled:opacity-50 aria-invalid:border-danger md:text-sm",
      Icon && "pl-8",
      clearable && value && "pr-8",
      className
    )}
    {type}
    bind:value
    {...rest} />
{/snippet}

{#if clearable || Icon}
  <span class={cn("relative block w-full min-w-0", wrapperClass)}>
    {#if Icon}
      <Icon
        class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-muted" />
    {/if}
    {@render field()}
    {#if clearable && value && !rest.disabled}
      <button
        type="button"
        class="absolute top-1/2 right-1.5 flex size-6 -translate-y-1/2 items-center justify-center rounded-badge text-fg-muted hover:bg-hover hover:text-fg"
        aria-label="Clear"
        onclick={clear}>
        <XIcon class="size-3.5" />
      </button>
    {/if}
  </span>
{:else}
  {@render field()}
{/if}

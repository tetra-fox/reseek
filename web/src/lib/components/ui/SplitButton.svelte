<script lang="ts" generics="Id extends string">
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import type { Component } from "svelte";

  import { viewport } from "#lib/ui/viewport.svelte.js";

  import Button from "./Button.svelte";
  import * as Menu from "./menu/menu.js";
  import Tooltip from "./Tooltip.svelte";

  type Props = {
    /** the choices; the chosen one names the main button and is what it does */
    options: readonly { id: Id; label: string }[];
    chosen: Id;
    icon: Component;
    disabled?: boolean;
    onclick: (id: Id) => void;
  };

  let { options, chosen = $bindable(), icon: Icon, disabled = false, onclick }: Props = $props();

  const label = $derived(options.find((o) => o.id === chosen)?.label ?? options[0].label);
</script>

<!-- an action with a scope picked from its menu; narrow screens show the icon with the words in a
     tooltip -->
<div class="inline-flex">
  <Tooltip text={viewport.lg ? undefined : label}>
    {#snippet child({ props })}
      <Button
        {...props}
        variant="outline"
        size="sm"
        class="rounded-r-none"
        {disabled}
        aria-label={label}
        onclick={() => onclick(chosen)}>
        <Icon />
        <span class="hidden lg:inline">{label}</span>
      </Button>
    {/snippet}
  </Tooltip>
  <Menu.Root>
    <Menu.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          variant="outline"
          size="icon-sm"
          class="rounded-l-none border-l-0"
          {disabled}
          aria-label={`Choose what ${label} acts on`}>
          <ChevronDownIcon />
        </Button>
      {/snippet}
    </Menu.Trigger>
    <Menu.Content align="end">
      <Menu.RadioGroup bind:value={chosen}>
        {#each options as option (option.id)}
          <Menu.RadioItem value={option.id}>{option.label}</Menu.RadioItem>
        {/each}
      </Menu.RadioGroup>
    </Menu.Content>
  </Menu.Root>
</div>

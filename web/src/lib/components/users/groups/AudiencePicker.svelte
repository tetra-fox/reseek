<script lang="ts">
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";

  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { audienceText } from "#lib/system/shares.js";

  // who something is for, picked from a menu: everyone, no one, or some of the groups. Ticking a
  // group while everyone is picked narrows it to that group; taking off the last group leaves no
  // one, never everyone
  type Props = {
    /** the groups picked, null for everyone and empty for no one */
    value: string[] | null;
    /** the groups that can be picked */
    choices: string[];
    onchange: (value: string[] | null) => void;
    /** what the menu picks for, as its button reads to a screen reader */
    label: string;
    disabled?: boolean;
    class?: string;
  };

  let { value, choices, onchange, label, disabled = false, class: className }: Props = $props();

  const toggle = (group: string, on: boolean) => {
    const held = value ?? [];
    onchange(on ? [...held, group] : held.filter((g) => g !== group));
  };
</script>

<Menu.Root>
  <Menu.Trigger {disabled}>
    {#snippet child({ props })}
      <Button
        {...props}
        variant="outline"
        size="sm"
        class={["justify-between", className]}
        aria-label={`${label}: ${audienceText(value)}`}>
        <span class="truncate">{audienceText(value)}</span><ChevronDownIcon />
      </Button>
    {/snippet}
  </Menu.Trigger>
  <Menu.Content align="start">
    <Menu.CheckboxItem
      checked={value === null}
      onCheckedChange={(on) => {
        if (on) onchange(null);
      }}>Everyone</Menu.CheckboxItem>
    <Menu.CheckboxItem
      checked={value?.length === 0}
      onCheckedChange={(on) => {
        if (on) onchange([]);
      }}>No one</Menu.CheckboxItem>
    <Menu.Separator />
    {#each choices as group (group)}
      <Menu.CheckboxItem
        checked={!!value?.includes(group)}
        onCheckedChange={(on) => toggle(group, on)}>{group}</Menu.CheckboxItem>
    {/each}
  </Menu.Content>
</Menu.Root>

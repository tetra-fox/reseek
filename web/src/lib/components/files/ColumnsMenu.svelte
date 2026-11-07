<script lang="ts">
  import Columns3Icon from "@lucide/svelte/icons/columns-3";

  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";

  // a list layout's Columns menu: which of its optional columns show, and a reset of its view
  type Props = {
    /** the columns that can be shown or hidden, in their order */
    columns: { id: string; label: string; visible: boolean }[];
    ontoggle: (id: string, visible: boolean) => void;
    /** puts the list's columns, widths and sort back as they start */
    onreset: () => void;
  };

  let { columns, ontoggle, onreset }: Props = $props();
</script>

<Menu.Root>
  <Menu.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="outline" size="sm"><Columns3Icon />Columns</Button>
    {/snippet}
  </Menu.Trigger>
  <Menu.Content align="start" class="min-w-48">
    {#each columns as column (column.id)}
      <Menu.CheckboxItem
        checked={column.visible}
        closeOnSelect={false}
        onCheckedChange={(on) => ontoggle(column.id, on)}>{column.label}</Menu.CheckboxItem>
    {/each}
    <Menu.Separator />
    <Menu.Item onSelect={onreset}>Reset view</Menu.Item>
  </Menu.Content>
</Menu.Root>

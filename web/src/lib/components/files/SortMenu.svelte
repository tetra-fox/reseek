<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import type { Snippet } from "svelte";

  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";

  type Dir = "asc" | "desc";

  // a list's Sort menu: what it orders by and which way. It reads and sets the same sort as the
  // list's column headers, so the two always agree
  type Props = {
    /** what the list sorts by; a field newly picked that sorts largest first starts that way */
    options: { id: string; label: string; descFirst?: boolean }[];
    /** the sort now, null while the list keeps its own order */
    sort: { id: string; dir?: Dir } | null;
    onsort: (sort: { id: string; dir: Dir }) => void;
    /** whether the sorts have a direction to choose; each of a tree's names its own */
    directed?: boolean;
    disabled?: boolean;
    /** items under the sorts, such as one that resets the view */
    children?: Snippet;
  };

  let { options, sort, onsort, directed = true, disabled, children }: Props = $props();

  const label = $derived(sort ? (options.find((o) => o.id === sort.id)?.label ?? sort.id) : null);
  const Icon = $derived(
    directed && sort?.dir ? (sort.dir === "asc" ? ArrowUpIcon : ArrowDownIcon) : ArrowUpDownIcon
  );

  /** A field picked starts the way it sorts first, as a column header's first click does */
  const pick = (id: string) => {
    if (sort?.id === id && sort.dir) return;
    onsort({ id, dir: options.find((o) => o.id === id)?.descFirst ? "desc" : "asc" });
  };
</script>

<Menu.Root>
  <Menu.Trigger {disabled}>
    {#snippet child({ props })}
      <Button {...props} variant="outline" size="sm">
        <Icon />{label ? `Sort: ${label}` : "Sort"}
      </Button>
    {/snippet}
  </Menu.Trigger>
  <Menu.Content align="start" class="min-w-48">
    <Menu.RadioGroup aria-label="Sort by" bind:value={() => sort?.id ?? "", pick}>
      {#each options as o (o.id)}
        <Menu.RadioItem value={o.id} closeOnSelect={false}>{o.label}</Menu.RadioItem>
      {/each}
    </Menu.RadioGroup>
    {#if directed}
      <Menu.Separator />
      <Menu.RadioGroup
        aria-label="Direction"
        bind:value={
          () => (sort ? (sort.dir ?? "") : ""),
          (dir) => {
            if (sort) onsort({ id: sort.id, dir: dir as Dir });
          }
        }>
        <Menu.RadioItem value="asc" closeOnSelect={false} disabled={!sort}
          >Ascending</Menu.RadioItem>
        <Menu.RadioItem value="desc" closeOnSelect={false} disabled={!sort}
          >Descending</Menu.RadioItem>
      </Menu.RadioGroup>
    {/if}
    {#if children}
      <Menu.Separator />
      {@render children()}
    {/if}
  </Menu.Content>
</Menu.Root>

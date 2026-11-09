<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import XIcon from "@lucide/svelte/icons/x";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import GroupBadge from "#lib/components/users/GroupBadge.svelte";
  import GroupDot from "#lib/components/users/GroupDot.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { setGroups, userGroups } from "#lib/users/actions.js";
  import { chosenGroups, withGroup, withoutGroup } from "#lib/users/groups.js";

  // the groups a user is in: the smart ones and the buddies as they are, and ours to add or take
  // them out of. A group the configuration no longer has shows dimmed
  type Props = { username: string; groups: Schemas["UserGroup"][]; disabled: boolean };
  let { username, groups, disabled }: Props = $props();

  const chosen = $derived(chosenGroups(groups));
  const addable = $derived(userGroups().filter((g) => !chosen.includes(g.name)));
  const admin = $derived(session.can("Administrator"));
  let saving = $state(false);

  const save = async (next: string[]) => {
    saving = true;
    const outcome = await setGroups(username, next);
    saving = false;
    if (!outcome.ok) toast(outcome.error, "error");
    return outcome.ok;
  };

  const add = (name: string) => void save(withGroup(chosen, name));

  /** Takes the user out of `name`, with an Undo that puts them back */
  const remove = async (name: string) => {
    const held = chosen;
    if (!(await save(withoutGroup(held, name)))) return;
    toast(`Removed ${username} from ${name}`, "info", {
      action: { label: "Undo", run: () => void save(held) }
    });
  };
</script>

<div class="flex flex-wrap items-center gap-1">
  {#each groups as group (group.name)}
    {@const ours = chosen.includes(group.name)}
    <GroupBadge {group} href={href(`/users?group=${encodeURIComponent(group.name)}`)}>
      {#if ours && !disabled}
        <button
          type="button"
          class="-mr-0.5 shrink-0 rounded-badge hover:bg-hover"
          aria-label={`Remove from ${group.name}`}
          disabled={saving}
          onclick={() => void remove(group.name)}><XIcon class="size-3" /></button>
      {/if}
    </GroupBadge>
  {/each}
  {#if !disabled && addable.length}
    <Menu.Root>
      <Menu.Trigger disabled={saving}>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="sm"><PlusIcon />Add to group</Button>
        {/snippet}
      </Menu.Trigger>
      <Menu.Content align="start">
        {#each addable as g (g.name)}
          <Menu.Item onSelect={() => add(g.name)}><GroupDot color={g.color} />{g.name}</Menu.Item>
        {/each}
      </Menu.Content>
    </Menu.Root>
  {:else if !disabled && !userGroups().length && admin}
    <a href={href("/users?tab=groups")} class="text-xs text-accent hover:underline">New group</a>
  {:else if !groups.length}
    <span class="text-xs text-fg-muted">In no group</span>
  {/if}
</div>

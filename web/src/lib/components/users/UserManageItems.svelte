<script lang="ts">
  import BanIcon from "@lucide/svelte/icons/ban";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { befriend, configBlocked, setBuddy } from "#lib/users/actions.js";
  import { toggleBlock } from "#lib/users/block.svelte.js";
  import { users } from "#lib/users/users.svelte.js";
  import { BUDDY, CONFIG_BLOCKED } from "#lib/wording.js";

  // a menu's buddy and block items, for a user `canManage` allows; a buddy also has the notify
  // choice
  let { name }: { name: string } = $props();

  const user = $derived(users.get(name));
  $effect(() => void users.load(name));

  const busy = new Busy();
  const write = (change: () => ReturnType<typeof befriend>) =>
    busy.run(async () => {
      const outcome = await change();
      if (outcome.ok) await users.load(name, { fresh: true });
      return outcome;
    });
</script>

<Menu.Item onSelect={() => write(() => setBuddy(name, !user?.buddy))}>
  {#if user?.buddy}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
</Menu.Item>
{#if user?.buddy}
  {@const buddy = user.buddy}
  <Tooltip text={BUDDY.notifyHint} side="right">
    {#snippet child({ props })}
      <Menu.CheckboxItem
        {...props}
        checked={buddy.notifyPresence}
        onCheckedChange={(on) => write(() => befriend(name, { notifyPresence: on }))}
        >{BUDDY.notify}</Menu.CheckboxItem>
    {/snippet}
  </Tooltip>
{/if}
<Tooltip text={configBlocked(user?.block) ? CONFIG_BLOCKED : undefined} side="right">
  {#snippet child({ props })}
    <Menu.Item
      {...props}
      disabled={configBlocked(user?.block)}
      onSelect={() => toggleBlock(name, !!user?.block)}>
      {#if user?.block}<ShieldCheckIcon />Unblock{:else}<BanIcon />Block{/if}
    </Menu.Item>
  {/snippet}
</Tooltip>

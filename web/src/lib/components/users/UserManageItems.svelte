<script lang="ts">
  import BanIcon from "@lucide/svelte/icons/ban";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { configBlocked, pin } from "#lib/users/actions.js";
  import { toggleBlock } from "#lib/users/block.svelte.js";
  import { users } from "#lib/users/users.svelte.js";
  import { BUDDY, CONFIG_BLOCKED } from "#lib/wording.js";

  // a menu's buddy and block items, for a user `canManage` allows
  let { name }: { name: string } = $props();

  const user = $derived(users.get(name));
  $effect(() => void users.load(name));

  const busy = new Busy();
  const togglePin = () =>
    busy.run(async () => {
      const outcome = await pin(name, !user?.pinned);
      if (outcome.ok) await users.load(name, { fresh: true });
      return outcome;
    });
</script>

<Menu.Item onSelect={togglePin}>
  {#if user?.pinned}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
</Menu.Item>
<Menu.Item
  disabled={configBlocked(user?.block)}
  title={configBlocked(user?.block) ? CONFIG_BLOCKED : undefined}
  onSelect={() => toggleBlock(name, !!user?.block)}>
  {#if user?.block}<ShieldCheckIcon />Unblock{:else}<BanIcon />Block{/if}
</Menu.Item>

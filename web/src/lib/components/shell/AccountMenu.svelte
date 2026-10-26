<script lang="ts">
  import { goto } from "$app/navigation";
  import CircleUserIcon from "@lucide/svelte/icons/circle-user";
  import EllipsisVerticalIcon from "@lucide/svelte/icons/ellipsis-vertical";
  import LogOutIcon from "@lucide/svelte/icons/log-out";

  import { app } from "#lib/app.svelte.js";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import { sidebar } from "./sidebar.svelte";

  const name = $derived(session.principal?.name ?? "");
  const role = $derived(session.principal?.role ?? "");
  let confirming = $state(false);

  const logout = async () => {
    const outcome = await session.logout();
    if (!outcome.ok) {
      toast(`Couldn't log out: ${outcome.error}`, "error");
      return;
    }
    // an identity provider's own logout page ends its session too
    if (outcome.data) window.location.href = outcome.data;
    else app.signedOut();
  };
</script>

<Menu.Root>
  <Tooltip text={sidebar.collapsed ? `${name} (${role})` : undefined} side="right">
    {#snippet child({ props })}
      <Menu.Trigger {...props}>
        {#snippet child({ props: trigger })}
          <button
            {...trigger}
            aria-label={sidebar.collapsed ? name : undefined}
            class={[
              "flex w-full items-center gap-2 rounded-control text-left text-sm transition-colors duration-(--motion-fast) select-none hover:bg-hover active:bg-press data-[state=open]:bg-hover",
              sidebar.collapsed ? "p-0" : "-mx-1 p-1"
            ]}>
            <span
              class="flex size-control shrink-0 items-center justify-center rounded-full bg-accent-soft text-xs font-semibold text-accent-soft-fg">
              {name.slice(0, 2).toUpperCase()}
            </span>
            {#if !sidebar.collapsed}
              <span class="grid flex-1 leading-tight">
                <span class="truncate font-medium">{name}</span>
                <span class="truncate text-xs text-fg-muted">{role}</span>
              </span>
              <EllipsisVerticalIcon class="size-4 shrink-0" />
            {/if}
          </button>
        {/snippet}
      </Menu.Trigger>
    {/snippet}
  </Tooltip>
  <Menu.Content
    class="w-(--bits-dropdown-menu-anchor-width) min-w-56"
    side={sidebar.phone ? "bottom" : "right"}
    align="end">
    <Menu.Item onSelect={() => goto(href("/system/account"))}>
      <CircleUserIcon />
      Account
    </Menu.Item>
    {#if session.canSignOut}
      <Menu.Separator />
      <Menu.Item onSelect={() => (confirming = true)}>
        <LogOutIcon />
        Log Out
      </Menu.Item>
    {/if}
  </Menu.Content>
</Menu.Root>

<ConfirmDialog
  bind:open={confirming}
  title="Confirm Log Out"
  body="Are you sure you want to log out?"
  confirm="Log Out"
  danger
  onconfirm={logout} />

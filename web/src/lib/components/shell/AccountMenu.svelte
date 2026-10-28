<script lang="ts">
  import { goto } from "$app/navigation";
  import CircleUserIcon from "@lucide/svelte/icons/circle-user";
  import ContactIcon from "@lucide/svelte/icons/contact";
  import EllipsisVerticalIcon from "@lucide/svelte/icons/ellipsis-vertical";
  import LogOutIcon from "@lucide/svelte/icons/log-out";

  import { app } from "#lib/app.svelte.js";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import { href } from "#lib/nav.js";
  import { ownProfile } from "#lib/profile.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import { sidebar } from "./sidebar.svelte";

  const name = $derived(session.principal?.name ?? "");
  const role = $derived(session.principal?.role ?? "");
  let confirming = $state(false);
  ownProfile.need();

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

<div class="flex items-center gap-1">
  {#if !sidebar.collapsed}
    <a
      href={href("/profile")}
      aria-label="Your profile"
      class="shrink-0 rounded-full transition-opacity hover:opacity-80">
      <Avatar {name} pictureUrl={ownProfile.data?.pictureUrl} class="size-control text-xs" />
    </a>
  {/if}
  <Menu.Root>
    <Tooltip text={sidebar.collapsed ? `${name} (${role})` : undefined} side="right">
      {#snippet child({ props })}
        <Menu.Trigger {...props}>
          {#snippet child({ props: trigger })}
            <button
              {...trigger}
              aria-label={sidebar.collapsed ? name : undefined}
              class={[
                "flex min-w-0 flex-1 items-center gap-2 rounded-control text-left text-sm transition-colors duration-(--motion-fast) hover:bg-hover active:bg-press data-[state=open]:bg-hover",
                sidebar.collapsed ? "p-0" : "p-1"
              ]}>
              {#if sidebar.collapsed}
                <Avatar
                  {name}
                  pictureUrl={ownProfile.data?.pictureUrl}
                  class="size-control text-xs" />
              {:else}
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
      <Menu.Item onSelect={() => goto(href("/profile"))}>
        <ContactIcon />
        Your profile
      </Menu.Item>
      <Menu.Item onSelect={() => goto(href("/system/account"))}>
        <CircleUserIcon />
        Account
      </Menu.Item>
      {#if session.canSignOut}
        <Menu.Separator />
        <Menu.Item onSelect={() => (confirming = true)}>
          <LogOutIcon />
          Log out
        </Menu.Item>
      {/if}
    </Menu.Content>
  </Menu.Root>
</div>

<ConfirmDialog
  bind:open={confirming}
  title="Log out?"
  body="You'll need to log in again to use reseek in this browser."
  confirm="Log out"
  danger
  onconfirm={logout} />

<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import BellIcon from "@lucide/svelte/icons/bell";
  import BellOffIcon from "@lucide/svelte/icons/bell-off";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import { api, byUser, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import GroupBadge from "#lib/components/users/GroupBadge.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, formatBytes, formatSpeed, plural } from "#lib/format.js";
  import { href, userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { assignGroup, pin, reach, userGroups } from "#lib/users/actions.js";
  import { blocking, unblock } from "#lib/users/block.svelte.js";
  import { nameColor } from "#lib/users/colors.js";
  import { flag } from "#lib/users/hover.svelte.js";
  import { BUDDY } from "#lib/wording.js";

  type Props = {
    profile: Schemas["UserProfile"];
    /** a refresh from the network is running */
    refreshing: boolean;
    onrefresh: () => void;
    /** something changed the user: read the profile again */
    onchanged: () => void;
  };

  let { profile: p, refreshing, onrefresh, onchanged }: Props = $props();

  const name = $derived(p.username);
  /** our own account, which can't be blocked */
  const own = $derived(app.state?.user.username === name);
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const stats = $derived(p.statistics);
  const country = $derived(flag(p.countryCode));
  const groups = $derived(userGroups());
  const lostGroup = $derived(!!p.groupOverride && !groups.includes(p.groupOverride));

  let deleting = $state(false);
  let purge = $state(false);

  /** Runs a write and reads the profile again after it, or says why it failed */
  const run = async (write: () => Promise<{ ok: boolean; error?: string }>) => {
    const outcome = await write();
    if (!outcome.ok) toast(outcome.error ?? "The request failed", "error");
    onchanged();
  };
  const mute = (on: boolean) =>
    call(() =>
      on
        ? api.PUT("/api/v0/users/{username}/mute", { params: byUser(name) })
        : api.DELETE("/api/v0/users/{username}/mute", { params: byUser(name) })
    );
  const remove = async () => {
    const outcome = await call(() =>
      api.DELETE("/api/v0/users/{username}", {
        params: { path: { username: name }, query: { ...byUser(name).query, purge } }
      })
    );
    if (!outcome.ok) return void toast(outcome.error, "error");
    deleting = false;
    toast(`Removed ${name} from the user list`);
    void goto(href("/users"));
  };
</script>

<section class="flex flex-col gap-4 rounded-card border bg-surface p-5 shadow-card">
  <div class="flex flex-wrap items-start gap-4">
    <Avatar {name} pictureUrl={p.info?.pictureUrl} class="size-16 text-2xl" />
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <div class="flex flex-wrap items-center gap-2">
        <span title={p.presenceAt ? `since ${ago(p.presenceAt, clock.now)}` : undefined}>
          <Presence presence={p.presence} />
        </span>
        <h1 class="truncate font-mono text-lg font-semibold" style:color={nameColor(name)}>
          {name}
        </h1>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Copy the username"
          onclick={() => void copyText(name)}><CopyIcon /></Button>
        {#if !p.known}<Badge tone="outline">Not seen before</Badge>{/if}
        {#if p.isPrivileged}<Badge tone="warning">privileged</Badge>{/if}
        {#if country}<span title={p.countryCode ?? ""}>{country}</span>{/if}
        <GroupBadge group={p.group} />
        {#if lostGroup}
          <Badge tone="warning" title={`This user is assigned to ${p.groupOverride}`}
            >Group no longer exists</Badge>
        {/if}
        {#if p.block}
          {@const b = p.block}
          <Tooltip
            text={[b.reason, b.expiresAt && `ends ${ago(b.expiresAt, clock.now)}`]
              .filter(Boolean)
              .join(", ") || undefined}
            hint>
            {#snippet child({ props })}
              <Badge {...props} tone="danger">blocked</Badge>
            {/snippet}
          </Tooltip>
        {/if}
        {#if p.pinned}<Badge tone="info">{BUDDY.badge}</Badge>{/if}
        {#each p.tags as tag (tag)}
          <a href={href(`/users?tag=${encodeURIComponent(tag)}`)}>
            <Badge tone="outline">{tag}</Badge>
          </a>
        {/each}
      </div>
      <!-- wraps as words do, so how long ago moves to the next line whole -->
      <p class="flex flex-wrap items-center gap-x-1.5 text-sm">
        {stats ? plural(stats.fileCount, "file", "files") : "? files"} ·
        {stats ? plural(stats.directoryCount, "folder", "folders") : "? folders"} ·
        {stats ? formatSpeed(stats.averageSpeed) : "?"} ·
        {stats ? plural(stats.uploadCount, "upload", "uploads") : "? uploads"}
        {#if refreshing}
          <Spinner class="size-3.5" label="Refreshing" />
        {:else if stats}
          <span class="text-xs whitespace-nowrap text-fg-subtle"
            >{ago(stats.fetchedAt, clock.now)}</span>
        {/if}
      </p>
      {#if p.known}
        <p class="text-xs text-fg-muted">
          {[
            p.firstSeenAt && `First seen ${ago(p.firstSeenAt, clock.now)}`,
            p.lastSeenAt && `last seen ${ago(p.lastSeenAt, clock.now)}`
          ]
            .filter(Boolean)
            .join(", ")}
        </p>
        <p class="text-xs text-fg-muted">
          {plural(p.figures.downloads.succeeded, "download", "downloads")} ({formatBytes(
            p.figures.downloads.bytes
          )}) · {plural(p.figures.uploads.succeeded, "upload", "uploads")} ({formatBytes(
            p.figures.uploads.bytes
          )}) · {plural(p.figures.messages.in + p.figures.messages.out, "message", "messages")}
        </p>
      {/if}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-inline" role="toolbar" aria-label="User actions">
    {#each reach(name, { only: ["message", "shares", "search"] }) as action (action.id)}
      <Button variant="outline" size="sm" href={action.href}><action.icon />{action.label}</Button>
    {/each}
    <Button variant="outline" size="sm" disabled={refreshing} onclick={onrefresh}
      ><RefreshCwIcon />Refresh</Button>
    {#if write && p.known}
      {#if own}
        <!-- our own account is never blocked -->
      {:else if p.block}
        <Button
          variant="outline"
          size="sm"
          disabled={p.block.source === "config"}
          title={p.block.source === "config" ? "Blocked by the configuration" : undefined}
          onclick={() => void unblock(name).then(onchanged)}><ShieldCheckIcon />Unblock</Button>
      {:else}
        <Button variant="outline" size="sm" onclick={() => blocking.ask(name)}
          ><BanIcon />Block</Button>
      {/if}
      <Button variant="outline" size="sm" onclick={() => void run(() => pin(name, !p.pinned))}>
        {#if p.pinned}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
      </Button>
      <Button variant="outline" size="sm" onclick={() => void run(() => mute(!p.muted))}>
        {#if p.muted}<BellIcon />Unmute{:else}<BellOffIcon />Mute{/if}
      </Button>
    {/if}
    {#if admin && p.known}
      <Menu.Root>
        <Tooltip text={p.group.source === "config" ? "Set in the configuration" : undefined}>
          {#snippet child({ props })}
            <span {...props}>
              <Menu.Trigger disabled={p.group.source === "config"}>
                {#snippet child({ props: trigger })}
                  <Button {...trigger} variant="outline" size="sm">Group<ChevronDownIcon /></Button>
                {/snippet}
              </Menu.Trigger>
            </span>
          {/snippet}
        </Tooltip>
        <Menu.Content align="start">
          <Menu.Item onSelect={() => void run(() => assignGroup(name, null))}>None</Menu.Item>
          {#each groups as g (g)}
            <Menu.Item onSelect={() => void run(() => assignGroup(name, g))}>{g}</Menu.Item>
          {/each}
        </Menu.Content>
      </Menu.Root>
    {/if}
    <Menu.Root>
      <Menu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label="More"
            ><EllipsisIcon /></Button>
        {/snippet}
      </Menu.Trigger>
      <Menu.Content align="start">
        <Menu.Item onSelect={() => void copyText(name)}><CopyIcon />Copy username</Menu.Item>
        <Menu.Item onSelect={() => window.open(userHref(name), "_blank", "noopener")}
          ><ExternalLinkIcon />Open in new tab</Menu.Item>
        {#if admin && p.known}
          <Menu.Separator />
          <Menu.Item onSelect={() => (deleting = true)}><Trash2Icon />Forget user</Menu.Item>
        {/if}
      </Menu.Content>
    </Menu.Root>
  </div>
</section>

<Dialog
  bind:open={deleting}
  title={`Forget ${name}?`}
  description="Removes the user from the user list, with their notes, tags and block. The user is listed again after any new interaction.">
  <label class="flex items-center gap-2 text-sm">
    <Checkbox checked={purge} onCheckedChange={(on) => (purge = on)} />
    Also remove the transfers and messages with this user
  </label>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleting = false)}>Cancel</Button>
    <Button variant="danger" onclick={() => void remove()}>Forget</Button>
  {/snippet}
</Dialog>

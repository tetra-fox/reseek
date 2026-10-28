<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import BellIcon from "@lucide/svelte/icons/bell";
  import BellOffIcon from "@lucide/svelte/icons/bell-off";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import { api, byUser, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { clock } from "#lib/clock.svelte.js";
  import { nameColour } from "#lib/colours.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, formatBytes, formatSpeed } from "#lib/format.js";
  import { href, userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import { assignGroup, links, pin, userGroups } from "./actions";
  import Avatar from "./Avatar.svelte";
  import { blocking, unblock } from "./block.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import { flag } from "./hover.svelte";
  import Presence from "./Presence.svelte";

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
  const to = $derived(links(name));
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
    if (!outcome.ok) toast(outcome.error ?? "Failed", "error");
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
    toast(`${name} is forgotten`);
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
        <h1 class="truncate font-mono text-lg font-semibold" style:color={nameColour(name)}>
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
          <Badge tone="warning" title={`The override names ${p.groupOverride}`}
            >group no longer exists</Badge>
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
        {#if p.pinned}<Badge tone="info">pinned</Badge>{/if}
        {#each p.tags as tag (tag)}
          <a href={href(`/users?tag=${encodeURIComponent(tag)}`)}>
            <Badge tone="outline">{tag}</Badge>
          </a>
        {/each}
      </div>
      <p class="flex items-center gap-1.5 text-sm">
        {stats ? stats.fileCount.toLocaleString() : "?"} files ·
        {stats ? stats.directoryCount.toLocaleString() : "?"} folders ·
        {stats ? formatSpeed(stats.averageSpeed) : "?"} ·
        {stats ? stats.uploadCount.toLocaleString() : "?"} uploads
        {#if refreshing}
          <Spinner class="size-3.5" label="Refreshing" />
        {:else if stats}
          <span class="text-xs text-fg-subtle">{ago(stats.fetchedAt, clock.now)}</span>
        {/if}
      </p>
      {#if p.known}
        <p class="text-xs text-fg-muted">
          {#if p.firstSeenAt}first seen {ago(p.firstSeenAt, clock.now)},{/if}
          {#if p.lastSeenAt}last seen {ago(p.lastSeenAt, clock.now)},{/if}
          {p.figures.downloads.succeeded} downloads ({formatBytes(p.figures.downloads.bytes)}) /
          {p.figures.uploads.succeeded} uploads ({formatBytes(p.figures.uploads.bytes)}),
          {p.figures.messages.in + p.figures.messages.out} messages
        </p>
      {/if}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-inline" role="toolbar" aria-label="User actions">
    {#if write}
      <Button variant="outline" size="sm" href={to.message}><MessageSquareIcon />Message</Button>
    {/if}
    <Button variant="outline" size="sm" href={userHref(name, "shares")}
      ><FolderOpenIcon />Browse</Button>
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
        {#if p.pinned}<PinOffIcon />Unpin{:else}<PinIcon />Pin{/if}
      </Button>
      <Button variant="outline" size="sm" onclick={() => void run(() => mute(!p.muted))}>
        {#if p.muted}<BellIcon />Unmute{:else}<BellOffIcon />Mute{/if}
      </Button>
    {/if}
    {#if admin && p.known}
      <Menu.Root>
        <Tooltip
          text={p.group.source === "config"
            ? "The configuration puts them in this group"
            : undefined}>
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
          <Menu.Item onSelect={() => (deleting = true)}><Trash2Icon />Delete user</Menu.Item>
        {/if}
      </Menu.Content>
    </Menu.Root>
  </div>
</section>

<Dialog
  bind:open={deleting}
  title={`Forget ${name}?`}
  description="Their directory entry, notes, tags and block go. They come back when you deal with them again.">
  <label class="flex items-center gap-2 text-sm">
    <Checkbox checked={purge} onCheckedChange={(on) => (purge = on)} />
    Also remove their transfers from the lists and delete our messages
  </label>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleting = false)}>Cancel</Button>
    <Button variant="danger" onclick={() => void remove()}>Delete</Button>
  {/snippet}
</Dialog>

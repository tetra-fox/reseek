<script lang="ts">
  import BanIcon from "@lucide/svelte/icons/ban";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserIcon from "@lucide/svelte/icons/user";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, formatSpeed } from "#lib/format.js";
  import { userHref } from "#lib/nav.js";
  import { firstLines, ownProfile } from "#lib/profile.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import type { UserSummary } from "#lib/users.svelte.js";

  import { links } from "./actions";
  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import { flag } from "./hover.svelte";

  type Props = {
    name: string;
    /** what the directory holds, undefined until it arrived */
    user: UserSummary | undefined;
    onblock: () => void;
  };

  let { name, user, onblock }: Props = $props();

  const to = $derived(links(name));
  const presence = $derived.by(() => {
    const p = user?.presence;
    if (!p) return { dot: "bg-fg-subtle/40", text: "Unknown" };
    if (p === "Online") return { dot: "bg-success", text: "Online" };
    if (p === "Away") {
      return {
        dot: "bg-warning",
        text: user?.presenceAt ? `Away ${ago(user.presenceAt)}` : "Away"
      };
    }
    return { dot: "bg-fg-subtle", text: "Offline" };
  });
  const stats = $derived(user?.statistics);
  const figures = $derived(user?.figures);
  const tags = $derived(user?.tags ?? []);
  // our own name shows what peers are sent now, from our profile
  const own = $derived(!!app.state?.user.username && app.state.user.username === name);
  $effect(() => {
    if (own) ownProfile.need();
  });
  const pictureUrl = $derived(own ? ownProfile.data?.pictureUrl : user?.info?.pictureUrl);
  const description = $derived(
    own
      ? ownProfile.data?.description
        ? firstLines(ownProfile.data.description, 3)
        : null
      : user?.description
  );
  const country = $derived(flag(user?.countryCode));
  const readWrite = $derived(session.can("ReadWrite"));
</script>

<div class="flex flex-col gap-3 text-sm">
  <div class="flex items-center gap-3">
    <Avatar {name} {pictureUrl} class="size-12" />
    <div class="flex min-w-0 flex-col">
      <span class="truncate font-mono font-medium">{name}</span>
      <span class="flex items-center gap-1.5 text-xs text-fg-muted">
        <span class={["size-2 rounded-full", presence.dot]}></span>
        {presence.text}
        {#if country}<span title={user?.countryCode ?? ""}>{country}</span>{/if}
      </span>
    </div>
  </div>

  {#if description}
    <p class="line-clamp-3 text-xs whitespace-pre-line text-fg">{description}</p>
  {/if}

  {#if !user}
    <Skeleton class="h-4 w-3/4" />
    <Skeleton class="h-4 w-1/2" />
  {:else}
    <div class="flex flex-wrap gap-1">
      <GroupBadge group={user.group} />
      {#if user.block}
        {@const block = user.block}
        <Tooltip
          text={[block.reason, block.expiresAt && `expires ${ago(block.expiresAt)}`]
            .filter(Boolean)
            .join(", ") || undefined}
          hint>
          {#snippet child({ props })}
            <Badge {...props} tone="danger">blocked</Badge>
          {/snippet}
        </Tooltip>
      {/if}
      {#if user.isPrivileged}<Badge tone="warning">privileged</Badge>{/if}
      {#if user.pinned}<Badge tone="info">pinned</Badge>{/if}
      {#each tags.slice(0, 3) as tag (tag)}
        <Badge tone="outline">{tag}</Badge>
      {/each}
      {#if tags.length > 3}<Badge tone="outline">+{tags.length - 3}</Badge>{/if}
    </div>

    <p class="text-xs">
      {stats ? stats.fileCount.toLocaleString() : "?"} files ·
      {stats ? stats.directoryCount.toLocaleString() : "?"} folders ·
      {stats ? formatSpeed(stats.averageSpeed) : "?"}
      {#if stats}<span class="text-fg-subtle">· {ago(stats.fetchedAt)}</span>{/if}
    </p>

    {#if figures}
      <p class="text-xs text-fg-muted">
        {figures.uploads.succeeded + figures.uploads.active} uploads ·
        {figures.downloads.succeeded + figures.downloads.active} downloads ·
        {figures.messages.in + figures.messages.out} messages
        {#if figures.messages.unread}({figures.messages.unread} unread){/if}
        {#if user.firstSeenAt}<br />first seen {ago(user.firstSeenAt)}{/if}
      </p>
    {/if}

    {#if user.notes}
      <p class="line-clamp-2 text-xs whitespace-pre-line text-fg-muted">
        <span class="font-medium">Note:</span>
        {user.notes}
      </p>
    {/if}
  {/if}

  <div class="flex gap-1 border-t pt-2">
    {#if readWrite}
      <Tooltip text="Message">
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" href={to.message} aria-label="Message">
            <MessageSquareIcon />
          </Button>
        {/snippet}
      </Tooltip>
    {/if}
    <Tooltip text="Browse shares">
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-sm"
          href={to.browse}
          aria-label="Browse shares">
          <FolderOpenIcon />
        </Button>
      {/snippet}
    </Tooltip>
    {#if readWrite}
      <Tooltip text={user?.block ? "Unblock" : "Block"}>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon-sm"
            aria-label={user?.block ? "Unblock" : "Block"}
            onclick={onblock}>
            {#if user?.block}<ShieldCheckIcon />{:else}<BanIcon />{/if}
          </Button>
        {/snippet}
      </Tooltip>
    {/if}
    <Tooltip text="Profile">
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-sm"
          href={userHref(name)}
          aria-label="Profile">
          <UserIcon />
        </Button>
      {/snippet}
    </Tooltip>
  </div>
</div>

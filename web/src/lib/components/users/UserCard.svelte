<script lang="ts">
  import BanIcon from "@lucide/svelte/icons/ban";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserIcon from "@lucide/svelte/icons/user";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import { app } from "#lib/app.svelte.js";
  import { Busy } from "#lib/busy.svelte.js";
  import { clock } from "#lib/clock.svelte.js";
  import { nameColour } from "#lib/colours.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, formatSpeed } from "#lib/format.js";
  import { userHref } from "#lib/nav.js";
  import { firstLines, ownProfile } from "#lib/profile.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { type UserSummary, users } from "#lib/users.svelte.js";
  import { BUDDY } from "#lib/wording.js";

  import { links, pin } from "./actions";
  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import { flag } from "./hover.svelte";

  type Props = {
    name: string;
    /** what the directory holds, undefined until it arrived */
    user: UserSummary | undefined;
    onblock: () => void;
    /** opens the name's menu, for the actions the card has no room for */
    onmenu?: () => void;
    /** a profile not saved yet, which the card shows as it would show for us */
    draft?: { description: string; src: string | null; pictureUrl: string | null };
  };

  let { name, user, onblock, onmenu, draft }: Props = $props();

  const to = $derived(links(name));
  // our own name shows what peers are sent now, from our profile
  const own = $derived(!!app.state?.user.username && app.state.user.username === name);
  // our own presence and share counts are the daemon's, not the directory's
  const presence = $derived.by(() => {
    const p = own ? (app.state?.server.isLoggedIn ? "Online" : "Offline") : user?.presence;
    if (!p) return { dot: "bg-fg-subtle/40", text: "Unknown" };
    if (p === "Online") return { dot: "bg-success", text: "Online" };
    if (p === "Away") {
      return {
        dot: "bg-warning",
        text: user?.presenceAt ? `Away ${ago(user.presenceAt, clock.now)}` : "Away"
      };
    }
    return {
      dot: "bg-fg-subtle",
      text: user?.presenceAt ? `Offline, last seen ${ago(user.presenceAt, clock.now)}` : "Offline"
    };
  });
  const stats = $derived(
    own && app.state
      ? { ...app.state.user.statistics, fetchedAt: null }
      : (user?.statistics ?? null)
  );
  const figures = $derived(user?.figures);
  const tags = $derived(user?.tags ?? []);
  $effect(() => {
    if (own) ownProfile.need();
  });
  const pictureUrl = $derived(
    draft ? draft.pictureUrl : own ? ownProfile.data?.pictureUrl : user?.info?.pictureUrl
  );
  const description = $derived.by(() => {
    const text = draft ? draft.description : own ? ownProfile.data?.description : null;
    return text === null || text === undefined ? user?.description : firstLines(text, 3) || null;
  });
  const country = $derived(flag(user?.countryCode));
  const readWrite = $derived(session.can("ReadWrite"));
  const pinning = new Busy();
  const togglePin = () =>
    pinning.run(async () => {
      const outcome = await pin(name, !user?.pinned);
      if (outcome.ok) await users.load(name, { fresh: true });
      else toast(outcome.error, "error");
      return outcome;
    });
</script>

<div class="flex flex-col gap-3 text-sm">
  <div class="flex items-center gap-3">
    <Avatar {name} {pictureUrl} src={draft?.src} class="size-12" />
    <div class="flex min-w-0 flex-col">
      <span class="truncate font-mono font-medium" style:color={nameColour(name)}>{name}</span>
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

  {#if !user && !own}
    <Skeleton class="h-4 w-3/4" />
    <Skeleton class="h-4 w-1/2" />
  {:else}
    {#if user}
      <div class="flex flex-wrap gap-1">
        <GroupBadge group={user.group} />
        {#if user.block}
          {@const block = user.block}
          <Tooltip
            text={[block.reason, block.expiresAt && `expires ${ago(block.expiresAt, clock.now)}`]
              .filter(Boolean)
              .join(", ") || undefined}
            hint>
            {#snippet child({ props })}
              <Badge {...props} tone="danger">blocked</Badge>
            {/snippet}
          </Tooltip>
        {/if}
        {#if user.isPrivileged}<Badge tone="warning">privileged</Badge>{/if}
        {#if user.pinned}<Badge tone="info">{BUDDY.badge}</Badge>{/if}
        {#each tags.slice(0, 3) as tag (tag)}
          <Badge tone="outline">{tag}</Badge>
        {/each}
        {#if tags.length > 3}<Badge tone="outline">+{tags.length - 3}</Badge>{/if}
      </div>
    {/if}

    <p class="text-xs">
      {stats ? stats.fileCount.toLocaleString() : "?"} files ·
      {stats ? stats.directoryCount.toLocaleString() : "?"} folders ·
      {stats ? formatSpeed(stats.averageSpeed) : "?"}
      {#if stats?.fetchedAt}<span class="text-fg-subtle">· {ago(stats.fetchedAt, clock.now)}</span
        >{/if}
    </p>

    {#if user && figures}
      <p class="text-xs text-fg-muted">
        {figures.uploads.succeeded + figures.uploads.active} uploads ·
        {figures.downloads.succeeded + figures.downloads.active} downloads ·
        {figures.messages.in + figures.messages.out} messages
        {#if figures.messages.unread}({figures.messages.unread} unread){/if}
        {#if user.firstSeenAt}<br />first seen {ago(user.firstSeenAt, clock.now)}{/if}
      </p>
    {/if}

    {#if user?.notes}
      <p class="line-clamp-2 text-xs whitespace-pre-line text-fg-muted">
        <span class="font-medium">Note:</span>
        {user.notes}
      </p>
    {/if}
  {/if}

  <div class="flex flex-wrap items-center gap-1 border-t pt-2">
    {#if readWrite && !own}
      <Button size="sm" href={to.message}><MessageSquareIcon />Message</Button>
    {/if}
    <Button size="sm" variant="outline" href={to.browse}><FolderOpenIcon />Browse</Button>
    {#if readWrite && !own}
      <Button
        size="sm"
        variant="outline"
        aria-pressed={!!user?.pinned}
        disabled={pinning.running}
        onclick={togglePin}>
        {#if user?.pinned}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
      </Button>
    {/if}
    <span class="ml-auto flex">
      {#if readWrite && !own}
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
      {#if onmenu}
        <Tooltip text="More">
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label="More" onclick={onmenu}>
              <EllipsisIcon />
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
    </span>
  </div>
</div>

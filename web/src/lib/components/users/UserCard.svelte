<script lang="ts">
  import BanIcon from "@lucide/svelte/icons/ban";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, chatTime, formatCount, formatSpeed, plural } from "#lib/format.js";
  import { userHref } from "#lib/nav.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { canManage, configBlocked, reach, setBuddy, USER_ACTIONS } from "#lib/users/actions.js";
  import { blockEnd } from "#lib/users/block.svelte.js";
  import { nameColor } from "#lib/users/colors.js";
  import { BUDDIES, queueOf } from "#lib/users/groups.js";
  import { firstLines, ownProfile, pictureOf, SHARES_UNKNOWN } from "#lib/users/profile.svelte.js";
  import { type UserSummary, users } from "#lib/users/users.svelte.js";
  import { BUDDY, CONFIG_BLOCKED } from "#lib/wording.js";

  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import NameText from "./NameText.svelte";

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
        text: user?.presenceAt ? `Away since ${chatTime(user.presenceAt, clock.now)}` : "Away"
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
  /** the groups beside the queue's, but those the badges before them already say */
  const queue = $derived(user ? queueOf(user) : null);
  const others = $derived.by(() => {
    const said = [user?.uploads.queue, "privileged", BUDDIES];
    return user?.groups.filter((g) => !said.includes(g.name)) ?? [];
  });
  $effect(() => {
    if (own) ownProfile.need();
  });
  const pictureUrl = $derived(draft ? draft.pictureUrl : pictureOf(name, user));
  const description = $derived.by(() => {
    const text = draft ? draft.description : own ? ownProfile.data?.description : null;
    return text === null || text === undefined ? user?.description : firstLines(text, 3) || null;
  });
  const manageable = $derived(canManage(name));
  const befriending = new Busy();
  const toggleBuddy = () =>
    befriending.run(async () => {
      const outcome = await setBuddy(name, !user?.buddy);
      if (outcome.ok) await users.load(name, { fresh: true });
      else toast(outcome.error, "error");
      return outcome;
    });
</script>

<div class="flex flex-col gap-3 text-sm">
  <div class="flex items-center gap-3">
    <!-- the face and the name go to the profile, as the Profile button does; a draft is our own
         profile being edited, which a link would leave -->
    <Avatar {name} {pictureUrl} src={draft?.src} link={!draft} class="size-12" />
    <div class="flex min-w-0 flex-col">
      {#if draft}
        <span class="font-mono font-medium wrap-anywhere" dir="auto" style:color={nameColor(name)}
          ><NameText {name} /></span>
      {:else}
        <a
          href={userHref(name)}
          class="font-mono font-medium wrap-anywhere underline-offset-2 hover:underline"
          dir="auto"
          aria-label={name}
          style:color={nameColor(name)}><NameText {name} /></a>
      {/if}
      <span class="flex items-center gap-1.5 text-xs text-fg-muted">
        <span class={["size-2 rounded-full", presence.dot]}></span>
        {presence.text}
        <Flag code={user?.countryCode} />
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
        {#if queue}<GroupBadge group={queue} queue />{/if}
        {#if user.block}
          {@const block = user.block}
          <Tooltip
            text={[block.reason, blockEnd(block, clock.now)].filter(Boolean).join(", ") ||
              undefined}
            spoken>
            {#snippet child({ props })}
              <Badge {...props} tone="danger">blocked</Badge>
            {/snippet}
          </Tooltip>
        {/if}
        {#if user.isPrivileged}<Badge tone="warning">privileged</Badge>{/if}
        {#if user.buddy}<Badge tone="info">{BUDDY.badge}</Badge>{/if}
        {#each others.slice(0, 3) as group (group.name)}
          <GroupBadge {group} />
        {/each}
        {#if others.length > 3}<Badge tone="outline">+{others.length - 3}</Badge>{/if}
      </div>
    {/if}

    <p class="text-xs">
      {#if stats}
        {plural(stats.fileCount, "file", "files")} ·
        {plural(stats.directoryCount, "folder", "folders")} ·
        {formatSpeed(stats.averageSpeed)}
        {#if stats.fetchedAt}<span class="text-fg-subtle">· {ago(stats.fetchedAt, clock.now)}</span
          >{/if}
      {:else}
        <span class="text-fg-muted">{SHARES_UNKNOWN}</span>
      {/if}
    </p>

    {#if user && figures}
      <p class="text-xs text-fg-muted">
        {plural(figures.uploads.succeeded + figures.uploads.active, "upload", "uploads")} ·
        {plural(figures.downloads.succeeded + figures.downloads.active, "download", "downloads")} ·
        {plural(figures.messages.in + figures.messages.out, "message", "messages")}
        {#if figures.messages.unread}({formatCount(figures.messages.unread)} unread){/if}
        {#if user.firstSeenAt}<br />First seen {ago(user.firstSeenAt, clock.now)}{/if}
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
    {#each reach(name, { only: ["message", "shares"] }) as action (action.id)}
      <Button size="sm" variant={action.id === "message" ? "primary" : "outline"} href={action.href}
        ><action.icon />{action.label}</Button>
    {/each}
    {#if manageable}
      <Button
        size="sm"
        variant="outline"
        aria-pressed={!!user?.buddy}
        disabled={befriending.running}
        onclick={toggleBuddy}>
        {#if user?.buddy}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
      </Button>
    {/if}
    <span class="ml-auto flex">
      {#if manageable}
        {@const byConfig = configBlocked(user?.block)}
        <Tooltip text={byConfig ? CONFIG_BLOCKED : user?.block ? "Unblock" : "Block"}>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={user?.block ? "Unblock" : "Block"}
              aria-disabled={byConfig || undefined}
              onclick={onblock}>
              {#if user?.block}<ShieldCheckIcon />{:else}<BanIcon />{/if}
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
      <Tooltip text={USER_ACTIONS.profile.label}>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon-sm"
            href={userHref(name)}
            aria-label={USER_ACTIONS.profile.label}>
            <USER_ACTIONS.profile.icon />
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

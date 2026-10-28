<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowUpFromLineIcon from "@lucide/svelte/icons/arrow-up-from-line";
  import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
  import CircleXIcon from "@lucide/svelte/icons/circle-x";
  import EraserIcon from "@lucide/svelte/icons/eraser";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import IdCardIcon from "@lucide/svelte/icons/id-card";
  import ListPlusIcon from "@lucide/svelte/icons/list-plus";
  import LockIcon from "@lucide/svelte/icons/lock";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { fly } from "svelte/transition";

  import {
    activity,
    activityPrefs,
    type ActivityEntry,
    type ActivityType
  } from "#lib/activity/activity.svelte.js";
  import { activityText, baseName, KINDS, kindOf } from "#lib/activity/text.js";
  import { clock } from "#lib/clock.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { ago, formatBytes, formatDate, shortTime } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { motionFast } from "#lib/motion.js";
  import { roomHref, userHref } from "#lib/nav.js";

  // what other users do with us on the network, as Nicotine+'s log pane shows it: a panel over the
  // bottom of every screen, opened from the footer
  $effect(() => activity.follow());
  keepCurrent(() => void activity.load());

  const hidden = $derived(new Set(activityPrefs.hidden));
  const shown = $derived(activity.shown.filter((e) => !hidden.has(kindOf(e.kind.type))));
  const toggle = (type: ActivityType) =>
    (activityPrefs.hidden = hidden.has(type)
      ? activityPrefs.hidden.filter((t) => t !== type)
      : [...activityPrefs.hidden, type]);

  const list = new ListControl<ActivityEntry>({
    lines: () => shown,
    key: (e) => String(e.id),
    id: (e) => String(e.id),
    // Enter opens the user's profile, or the room for a room's change
    onopen: (e) => {
      if (e.username) void goto(userHref(e.username));
      else if (e.kind.type === "privateRoom") void goto(roomHref(e.kind.room));
    }
  });

  const ICONS = {
    browse: FolderOpenIcon,
    profile: IdCardIcon,
    search: SearchIcon,
    queued: ListPlusIcon,
    uploadStarted: ArrowUpFromLineIcon,
    uploadEnded: CircleCheckIcon,
    privateRoom: LockIcon
  } as const;
</script>

{#snippet file(path: string)}<span class="font-medium" title={path}>{baseName(path)}</span
  >{/snippet}
{#snippet who(name: string | null | undefined)}{#if name}<Username {name} />{/if}{/snippet}

<section
  aria-label="Activity on the network"
  class="fixed inset-x-0 bottom-10 z-20 flex h-72 max-h-[45svh] flex-col border-t bg-surface/95 shadow-raised backdrop-blur-md"
  transition:fly={{ y: 24, duration: motionFast() }}>
  <header class="flex flex-wrap items-center gap-2 border-b px-gutter py-1.5">
    <h2 class="text-sm font-semibold">Activity</h2>
    <span class="text-xs text-fg-muted">what other users do with you, since reseek started</span>
    <div class="ml-auto flex flex-wrap items-center gap-1" role="group" aria-label="Kinds shown">
      {#each KINDS as k (k.type)}
        <button
          type="button"
          aria-pressed={!hidden.has(k.type)}
          class="h-6 rounded-full border px-2 text-xs text-fg-subtle line-through transition-colors duration-(--motion-fast) hover:text-fg aria-pressed:border-transparent aria-pressed:bg-surface-muted aria-pressed:text-fg-muted aria-pressed:no-underline"
          onclick={() => toggle(k.type)}>{k.label}</button>
      {/each}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Clear the view"
        title="Clear the view"
        onclick={() => activity.clearView()}><EraserIcon /></Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Close the activity"
        onclick={() => (activityPrefs.open = false)}><XIcon /></Button>
    </div>
  </header>
  {#if activity.error}
    <Notice kind="error">
      {activity.error}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => void activity.load()}>Retry</Button>
      {/snippet}
    </Notice>
  {/if}
  <div
    class="min-h-0 flex-1 overflow-y-auto px-gutter py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
    role="grid"
    aria-label="Activity"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    {#each shown as e, i (e.id)}
      {@const k = e.kind}
      {@const Icon =
        k.type === "uploadEnded" && k.state !== "Completed, Succeeded"
          ? CircleXIcon
          : ICONS[k.type]}
      <div
        role="row"
        {...list.line(e, i)}
        aria-label={activityText(e)}
        class={[
          "grid grid-cols-[4.5rem_1.25rem_minmax(0,1fr)] items-center gap-2 rounded-badge px-1 py-0.5 hover:bg-hover",
          list.selected.has(String(e.id)) && "bg-accent-soft/60",
          list.isFocused(e) && "ring-1 ring-accent ring-inset"
        ]}>
        <span role="gridcell" class="text-xs text-fg-subtle tabular-nums"
          ><time datetime={e.at} title={`${formatDate(e.at)}, ${ago(e.at, clock.now)}`}
            >{shortTime(e.at)}</time
          ></span>
        <span role="gridcell" class="flex justify-center">
          <Icon
            class={[
              "size-3.5",
              k.type === "uploadEnded"
                ? k.state === "Completed, Succeeded"
                  ? "text-success"
                  : "text-danger"
                : "text-fg-muted"
            ]} />
        </span>
        <span role="gridcell" class="min-w-0 truncate">
          {#if k.type === "browse"}
            {@render who(e.username)} is browsing your shares
          {:else if k.type === "profile"}
            {@render who(e.username)} is viewing your profile
          {:else if k.type === "search"}
            {@render who(e.username)} searched for
            <span class="font-medium">"{k.query}"</span>
            and got {k.results === 1 ? "1 result" : `${k.results.toLocaleString()} results`}
          {:else if k.type === "queued"}
            {@render who(e.username)} queued
            <span class="font-medium tabular-nums"
              >{k.files === 1 ? "1 file" : `${k.files} files`}</span
            ><span class="text-fg-muted">, the last {@render file(k.filename)}</span>
          {:else if k.type === "uploadStarted"}
            Uploading {@render file(k.filename)}
            <span class="text-fg-muted">({formatBytes(k.size)})</span> to {@render who(e.username)}
          {:else if k.type === "uploadEnded"}
            {#if k.state === "Completed, Succeeded"}
              Finished uploading {@render file(k.filename)} to {@render who(e.username)}
            {:else}
              Upload of {@render file(k.filename)} to {@render who(e.username)} ended:
              <span class="text-danger">{k.state.replace("Completed, ", "")}</span>
              <span class="text-fg-muted">after {formatBytes(k.bytes)}</span>
            {/if}
          {:else if k.type === "privateRoom"}
            {@const room = k.room}
            {#snippet link()}<a
                class="font-medium text-accent hover:underline"
                href={roomHref(room)}>{room}</a
              >{/snippet}
            {#if k.change === "added"}You were added to the private room {@render link()}
            {:else if k.change === "removed"}You were removed from the private room {@render link()}
            {:else if k.change === "operator"}You are an operator of {@render link()} now
            {:else}You are no longer an operator of {@render link()}{/if}
          {/if}
        </span>
      </div>
    {:else}
      <EmptyState
        icon={FolderOpenIcon}
        title="Nothing yet"
        description="When someone browses your shares, gets your search results or queues a file, it shows here." />
    {/each}
  </div>
</section>

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
  import { fly, slide } from "svelte/transition";

  import {
    activity,
    activityPrefs,
    type ActivityEntry,
    type ActivityType,
    DOCK_MIN,
    dockMax
  } from "#lib/activity/activity.svelte.js";
  import { activityParts, activityText, baseName, KINDS, kindOf } from "#lib/activity/text.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import LogTime from "#lib/components/ui/LogTime.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import ResizeHandle from "#lib/components/ui/ResizeHandle.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatBytes } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { motionFast } from "#lib/motion.js";
  import { roomHref, userHref } from "#lib/nav.js";
  import { viewport } from "#lib/viewport.svelte.js";

  // what other users do with us on the network: a panel at the bottom of every screen, opened
  // from the footer or with Ctrl+J, so it stays in view whatever screen is open
  $effect(() => activity.follow());

  /** the docked height: the remembered one, or less in a window too short for it */
  let height = $state(Math.min(activityPrefs.height, dockMax()));
  /** docked it opens and closes by its height, which moves the content with it; a sheet rises */
  const drawer = (node: Element) =>
    viewport.phone
      ? fly(node, { y: 24, duration: motionFast() })
      : slide(node, { duration: motionFast() });
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

<!-- on a wide screen the log is a panel in the shell, above the footer, that takes its height
     from the content area and slides it up as it opens; a phone has no room to spare for one,
     so there it is a sheet over the bottom of the screen, standing on the footer at the height
     the shell measures for it -->
<section
  aria-label="Activity on the network"
  class={[
    "flex flex-col border-t",
    viewport.phone
      ? "fixed inset-x-0 bottom-(--bottom-h) z-20 h-72 max-h-[45svh] bg-surface/95 shadow-raised backdrop-blur-md"
      : "relative bg-surface"
  ]}
  style:height={viewport.phone ? undefined : `${height}px`}
  transition:drawer|global>
  {#if !viewport.phone}
    <ResizeHandle
      bind:value={height}
      min={DOCK_MIN}
      max={dockMax()}
      edge="top"
      label="Height of the activity log"
      onchange={(h) => (activityPrefs.height = h)}
      class="absolute inset-x-0 -top-1 z-10 h-2" />
  {/if}
  <header class="flex flex-wrap items-center gap-2 border-b px-gutter py-1.5">
    <h2 class="text-sm font-semibold">Activity</h2>
    <div class="ml-auto flex flex-wrap items-center gap-1" role="group" aria-label="Activity types">
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
        aria-label="Clear the list"
        title="Clear the list"
        onclick={() => activity.clearView()}><EraserIcon /></Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Close the activity log"
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
        aria-label={activityText(e, formatBytes)}
        class={["log-row", list.selected.has(String(e.id)) && "bg-accent-soft/60"]}>
        <LogTime at={e.at} />
        <span role="gridcell" class="flex justify-center pt-1">
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
          {#each activityParts(e, formatBytes) as part, n (n)}{#if part.kind === "user"}<Username
                name={part.name} />{:else if part.kind === "file"}<span
                class="font-medium"
                title={part.path}>{baseName(part.path)}</span
              >{:else if part.kind === "room"}<a
                class="font-medium text-accent hover:underline"
                href={roomHref(part.name)}>{part.name}</a
              >{:else if part.kind === "strong"}<span class="font-medium tabular-nums"
                >{part.text}</span
              >{:else if part.kind === "failed"}<span class="text-danger">{part.text}</span
              >{:else if part.kind === "muted"}<span class="text-fg-muted">{part.text}</span
              >{:else}{part.text}{/if}{/each}
        </span>
      </div>
    {:else}
      <EmptyState icon={FolderOpenIcon} title="No activity yet" />
    {/each}
  </div>
</section>

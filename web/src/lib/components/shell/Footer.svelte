<script lang="ts">
  import ActivityIcon from "@lucide/svelte/icons/activity";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";

  import { activityPrefs } from "#lib/activity/activity.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import { formatSpeedPadded } from "#lib/format.js";
  import { REPOSITORY } from "#lib/project.js";
  import { shortcutAria, shortcutText } from "#lib/ui/keys.js";

  const connected = $derived(app.isServerConnected);
  const username = $derived(app.state?.user.username);

  /** The figures of one direction, none when no figures came yet */
  const figures = (direction: "downloads" | "uploads") => {
    const m = app.metrics?.[direction];
    return {
      speed: formatSpeedPadded(m?.inProgress.totalSpeed),
      active: m?.inProgress.files ?? 0,
      queued: m?.queued.files ?? 0
    };
  };
  const downloads = $derived(figures("downloads"));
  const uploads = $derived(figures("uploads"));
</script>

<!-- the server connection and the transfer figures, always in view. A phone keeps the speeds
     and the activity log's button and leaves out the counts and the project link. Where a large
     text size leaves no room, the speeds move to a second line together -->
<footer
  class="relative z-20 flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1 border-t bg-surface/90 px-gutter py-1.5 text-xs backdrop-blur-md md:gap-x-4">
  <span class="flex min-w-0 items-center gap-1.5">
    <span class={["size-2 shrink-0 rounded-full", connected ? "bg-success" : "bg-danger"]}></span>
    <span class="truncate font-medium">{connected && username ? username : "Disconnected"}</span>
  </span>
  <span class="flex shrink-0 items-center gap-3 md:gap-4">
    <span class="flex shrink-0 items-center gap-1.5" aria-label="Downloads">
      <ArrowDownIcon class="size-3.5 text-info" />
      <span class="font-mono whitespace-pre tabular-nums">{downloads.speed}</span>
      <span class="text-fg-muted max-md:hidden"
        >{downloads.active} active · {downloads.queued} queued</span>
    </span>
    <span class="flex shrink-0 items-center gap-1.5" aria-label="Uploads">
      <ArrowUpIcon class="size-3.5 text-success" />
      <span class="font-mono whitespace-pre tabular-nums">{uploads.speed}</span>
      <span class="text-fg-muted max-md:hidden"
        >{uploads.active} active · {uploads.queued} queued</span>
    </span>
  </span>
  {#if app.mode !== "agent"}
    <button
      type="button"
      aria-pressed={activityPrefs.open}
      class="flex shrink-0 items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-fg-muted hover:bg-hover hover:text-fg aria-pressed:bg-surface-muted aria-pressed:text-fg max-md:ml-auto"
      title={`Activity log: browses, searches and downloads by other users (${shortcutText("J")})`}
      aria-keyshortcuts={shortcutAria("J")}
      onclick={() => (activityPrefs.open = !activityPrefs.open)}>
      <ActivityIcon class="size-3.5" /><span class="max-md:sr-only">Activity</span>
    </button>
  {/if}
  <a
    href={REPOSITORY}
    target="_blank"
    rel="noreferrer"
    class="ml-auto flex shrink-0 items-center gap-1.5 text-fg-muted hover:text-fg max-md:hidden">
    <img src="./favicon.svg" width="18" height="18" alt="" />
    <span class="font-medium">reseek</span>
    <span class="font-mono opacity-70">{app.state?.version.current ?? ""}</span>
    <span class="font-mono opacity-50">AGPL-3.0</span>
  </a>
</footer>

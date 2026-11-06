<script lang="ts">
  import ActivityIcon from "@lucide/svelte/icons/activity";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ListChecksIcon from "@lucide/svelte/icons/list-checks";

  import { activityPrefs } from "#lib/activity/activity.svelte.js";
  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { formatSpeedPadded } from "#lib/format.js";
  import { failureUnseen, summaryText } from "#lib/jobs/jobs.js";
  import { jobsSeen } from "#lib/jobs/jobs.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { REPOSITORY } from "#lib/shell/project.js";
  import { shortcutAria, shortcutText } from "#lib/ui/keys.js";

  import SpeedLimits from "./SpeedLimits.svelte";

  const connected = $derived(app.isServerConnected);
  const username = $derived(app.state?.user.username);
  const away = $derived(app.state?.user.status === "Away");
  const canSet = $derived(session.can("ReadWrite"));
  const setting = new Busy();
  const setStatus = () =>
    setting.run(() =>
      call(() =>
        api.PUT("/api/v0/application/status", {
          body: { status: away ? "Online" : "Away" }
        })
      )
    );

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
  // the jobs running or paused, and a mark for a failure since the Jobs page was last opened
  const jobsText = $derived(summaryText(app.state?.jobs));
  const jobsShort = $derived(summaryText(app.state?.jobs, true));
  const failed = $derived(failureUnseen(app.state?.jobs, jobsSeen.at));
  const uploads = $derived(figures("uploads"));
</script>

<!-- the server connection and the transfer figures, always in view. A phone keeps the speeds
     and the activity log's button and leaves out the counts and the project link. Where a large
     text size leaves no room, the speeds move to a second line together -->
<footer
  class="relative z-20 flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1 border-t bg-surface/90 px-gutter py-1.5 text-xs backdrop-blur-md md:gap-x-4">
  <!-- our name, and whether others see us online or away, which a click changes -->
  <Tooltip
    text={!canSet
      ? undefined
      : !connected
        ? away
          ? "You will show as away once connected. Click to show as online."
          : "Click to show as away once connected"
        : away
          ? "Others see you as away. Click to show as online."
          : "Others see you as online. Click to show as away."}>
    {#snippet child({ props })}
      <button
        {...props}
        type="button"
        disabled={!canSet || setting.running}
        class="-mx-1 flex h-control-sm min-w-0 items-center gap-1.5 rounded-badge px-1 enabled:hover:bg-hover"
        onclick={() => void setStatus()}>
        <span
          class={[
            "size-2 shrink-0 rounded-full",
            !connected ? "bg-danger" : away ? "bg-warning" : "bg-success"
          ]}></span>
        <span class="truncate font-medium"
          >{connected && username ? username : "Disconnected"}</span>
        {#if away}<span class="shrink-0 text-fg-muted">Away</span>{/if}
      </button>
    {/snippet}
  </Tooltip>
  <SpeedLimits>
    <span class="flex shrink-0 items-center gap-1.5" aria-label="Downloads">
      <ArrowDownIcon class="size-3.5 text-download" />
      <span class="font-mono whitespace-pre tabular-nums">{downloads.speed}</span>
      <span class="text-fg-muted max-md:hidden"
        >{downloads.active} active · {downloads.queued} queued</span>
    </span>
    <span class="flex shrink-0 items-center gap-1.5" aria-label="Uploads">
      <ArrowUpIcon class="size-3.5 text-upload" />
      <span class="font-mono whitespace-pre tabular-nums">{uploads.speed}</span>
      <span class="text-fg-muted max-md:hidden"
        >{uploads.active} active · {uploads.queued} queued</span>
    </span>
  </SpeedLimits>
  {#if jobsText || failed}
    <a
      href={href("/jobs")}
      class="flex h-control-sm shrink-0 items-center gap-1.5 rounded-badge px-2 text-fg-muted hover:bg-hover hover:text-fg"
      title={failed ? "A job failed" : undefined}>
      <ListChecksIcon class="size-3.5" />
      <span class="max-md:hidden">{jobsText ?? "A job failed"}</span>
      <span class="md:hidden">{jobsShort ?? "A job failed"}</span>
      {#if failed}<span class="size-2 shrink-0 rounded-full bg-danger" aria-label="A job failed"
        ></span
        >{/if}
    </a>
  {/if}
  <button
    type="button"
    aria-pressed={activityPrefs.open}
    class="flex h-control-sm shrink-0 items-center gap-1.5 rounded-badge px-2 text-fg-muted hover:bg-hover hover:text-fg aria-pressed:bg-surface-muted aria-pressed:text-fg max-md:ml-auto"
    title={`Activity log: browses, searches and downloads by other users (${shortcutText("J")})`}
    aria-keyshortcuts={shortcutAria("J")}
    onclick={() => (activityPrefs.open = !activityPrefs.open)}>
    <ActivityIcon class="size-3.5" /><span class="max-md:sr-only">Activity</span>
  </button>
  <a
    href={REPOSITORY}
    target="_blank"
    rel="noreferrer"
    class="ml-auto flex h-control-sm shrink-0 items-center gap-1.5 text-fg-muted hover:text-fg max-md:hidden">
    <img src="./favicon.svg" width="18" height="18" alt="" />
    <span class="font-medium">reseek</span>
    <span class="font-mono opacity-70">{app.state?.version.current ?? ""}</span>
    <span class="font-mono opacity-50">AGPL-3.0</span>
  </a>
</footer>

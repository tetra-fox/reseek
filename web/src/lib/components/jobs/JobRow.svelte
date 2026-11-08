<script lang="ts">
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import FolderSyncIcon from "@lucide/svelte/icons/folder-sync";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import WrenchIcon from "@lucide/svelte/icons/wrench";
  import XIcon from "@lucide/svelte/icons/x";
  import type { Component } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import {
    endedBadly,
    fraction,
    type Job,
    type JobKind,
    KIND_LABELS,
    progressText,
    starterText,
    stateText,
    toneOf,
    tookText
  } from "#lib/jobs/jobs.js";
  import { jobs } from "#lib/jobs/jobs.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { PAUSE_WARNING } from "#lib/transfers/bulk.js";
  import { disclose } from "#lib/ui/motion.js";

  import JobParts from "./JobParts.svelte";

  // one job: what it is, where it is and how far, who started it, its controls, and its parts
  // once opened
  let { job, open, ontoggle }: { job: Job; open: boolean; ontoggle: () => void } = $props();

  const ICONS: Record<JobKind, Component> = {
    download: DownloadIcon,
    scan: FolderSyncIcon,
    analysis: AudioLinesIcon,
    maintenance: WrenchIcon
  };
  const Icon = $derived(ICONS[job.kind]);

  const write = $derived(session.can("ReadWrite"));
  const busy = new Busy();
  const ended = $derived(job.state === "ended");
  // a running scan says how far it is in the state, not in its job
  const done = $derived(
    job.kind === "scan" && !ended ? (app.state?.shares.scanProgress ?? null) : fraction(job)
  );
  const counted = $derived(progressText(job));

  const pause = () => {
    const run = () => void busy.run(() => jobs.control(job.id, "pause"));
    if (job.kind !== "download") return run();
    confirmation.ask({
      title: `Pause ${job.name}?`,
      body: PAUSE_WARNING,
      confirm: "Pause",
      run
    });
  };
  const cancel = () =>
    confirmation.ask({
      title: `Cancel ${job.name}?`,
      body: job.kind === "download" ? "Unfinished files stop. Finished files stay." : undefined,
      confirm: "Cancel job",
      dismiss: "Keep going",
      run: () => void busy.run(() => jobs.control(job.id, "cancel"))
    });
</script>

<!-- the columns are the job list's, taken through subgrids -->
<div class="col-span-full grid grid-cols-subgrid">
  <div class="col-span-full grid grid-cols-subgrid items-center gap-y-1 px-4 py-2.5">
    <!-- a phone gives the name the row's width, with the state, the time and the actions under it -->
    {#if job.counts.total}
      <Tooltip text={open ? "Hide parts" : "Show parts"}>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class="flex size-control-sm items-center justify-center rounded-control text-fg-muted hover:bg-hover hover:text-fg"
            aria-expanded={open}
            aria-label={open ? `Hide parts of ${job.name}` : `Show parts of ${job.name}`}
            onclick={ontoggle}>
            <ChevronRightIcon
              class={[
                "size-4 transition-transform duration-(--motion-fast)",
                open && "rotate-90"
              ]} />
          </button>
        {/snippet}
      </Tooltip>
    {:else}
      <span class="size-control-sm"></span>
    {/if}
    <div class="flex min-w-0 flex-col gap-0.5">
      <span class="flex min-w-0 items-center gap-2">
        <Icon class="size-4 shrink-0 text-fg-muted" role="img" aria-label={KIND_LABELS[job.kind]} />
        <Tooltip text={job.name} clipped>
          {#snippet child({ props })}
            <span {...props} class="truncate font-medium">{job.name}</span>
          {/snippet}
        </Tooltip>
      </span>
      <span class="truncate text-xs text-fg-muted">{starterText(job, jobs.schedules)}</span>
    </div>
    <div class="flex min-w-0 flex-col gap-1 max-md:col-start-2">
      <span class="flex flex-wrap items-center gap-1.5">
        <Badge tone={toneOf(job)}
          >{#if job.state === "running"}<Spinner class="size-3" label="Running" />{/if}{stateText(
            job
          )}</Badge>
        {#if counted}<span class="text-xs text-fg-muted tabular-nums">{counted}</span>{/if}
      </span>
      {#if !ended && done !== null}
        <Progress
          value={done * 100}
          label={`Progress of ${job.name}`}
          class="w-full max-w-48 ring-1 ring-border" />
      {/if}
      {#if job.error}
        <Tooltip text={job.error} clipped>
          {#snippet child({ props })}
            <span {...props} class="line-clamp-2 text-xs text-danger">{job.error}</span>
          {/snippet}
        </Tooltip>
      {:else if job.summary}
        <Tooltip text={job.summary} clipped>
          {#snippet child({ props })}
            <span {...props} class="truncate text-xs text-fg-muted">{job.summary}</span>
          {/snippet}
        </Tooltip>
      {/if}
    </div>
    <div class="flex flex-col gap-0.5 text-xs text-fg-muted max-md:col-start-2">
      {#if job.endedAt}
        <span
          >Ended <Time at={job.endedAt} />{#if job.startedAt}
            (took {tookText(job.startedAt, job.endedAt)}){/if}</span>
      {:else if job.startedAt}
        <span>Started <Time at={job.startedAt} /></span>
      {:else}
        <span>Queued <Time at={job.createdAt} /></span>
      {/if}
    </div>
    <div class="flex items-center gap-1 row-action max-md:col-start-2 md:justify-end">
      {#if job.kind === "download"}
        <Button variant="ghost" size="sm" href={href(`/downloads?job=${job.id}`)}
          >Show in Downloads</Button>
      {/if}
      {#if write}
        {#if job.pausable && job.state === "paused"}
          <Tooltip text="Resume">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Resume"
                disabled={busy.running}
                onclick={() => void busy.run(() => jobs.control(job.id, "resume"))}
                ><PlayIcon /></Button>
            {/snippet}
          </Tooltip>
        {:else if job.pausable}
          <Tooltip text="Pause">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Pause"
                disabled={busy.running}
                onclick={pause}><PauseIcon /></Button>
            {/snippet}
          </Tooltip>
        {/if}
        {#if !ended}
          <Tooltip text="Cancel">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Cancel"
                disabled={busy.running}
                onclick={cancel}><XIcon /></Button>
            {/snippet}
          </Tooltip>
        {:else}
          {#if endedBadly(job)}
            <Tooltip
              text={job.kind === "download"
                ? "Queue missing files again"
                : job.kind === "analysis"
                  ? "Analyze failed files again"
                  : "Run again"}>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Run again"
                  disabled={busy.running}
                  onclick={() => void busy.run(() => jobs.runAgain(job.id), "Started again")}
                  ><RotateCwIcon /></Button>
              {/snippet}
            </Tooltip>
          {/if}
          <Tooltip text="Remove from list">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Remove from list"
                disabled={busy.running}
                onclick={() => void busy.run(() => jobs.remove(job.id))}><Trash2Icon /></Button>
            {/snippet}
          </Tooltip>
        {/if}
      {/if}
    </div>
  </div>
  {#if open && job.counts.total}
    <div
      class="col-span-full border-t bg-surface-muted/50 px-4 py-2 pl-14 text-xs"
      transition:disclose>
      <JobParts {job} />
    </div>
  {/if}
</div>

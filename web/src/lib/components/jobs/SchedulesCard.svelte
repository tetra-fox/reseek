<script lang="ts">
  import PlayIcon from "@lucide/svelte/icons/play";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { ENDING_LABELS, everyText, type Schedule, tookText } from "#lib/jobs/jobs.js";
  import { jobs } from "#lib/jobs/jobs.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  // the recurring work: what each schedule does, how often, its last and its next run, and Run
  // now; an off schedule is greyed with a link to the setting that turns it on
  let { onshow }: { onshow: (id: string) => void } = $props();

  const write = $derived(session.can("ReadWrite"));
  const busy = new Busy();
  const run = (s: Schedule) => void busy.run(() => jobs.runSchedule(s.name), `${s.title} started`);
  const settingHref = (option: string) => href(`/system/settings?q=${encodeURIComponent(option)}`);
  const tone = (s: Schedule) =>
    s.lastEnding === "succeeded"
      ? "success"
      : s.lastEnding === "finished_with_failures"
        ? "warning"
        : s.lastEnding === "failed"
          ? "danger"
          : "neutral";
</script>

<Card title="Scheduled jobs">
  {#if jobs.schedulesError && !jobs.schedules.length}
    <ErrorSegment caption={jobs.schedulesError} retry={() => void jobs.load()} retryLabel="Retry" />
  {/if}
  <!-- the rows share the list's columns, so a row without Run now lines up with the others -->
  <ul
    data-edge
    class="-mx-5 grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 divide-y border-y text-sm md:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1fr)_auto]">
    {#each jobs.schedules as s (s.name)}
      {@const off = !s.every}
      <li
        class={[
          "col-span-full grid grid-cols-subgrid items-start gap-y-1 px-5 py-2.5",
          off && "text-fg-muted"
        ]}>
        <div class="flex min-w-0 flex-col gap-0.5">
          <span class="font-medium">{s.title}</span>
          <span class="text-xs text-fg-muted">{s.description}</span>
        </div>
        <div class="flex min-w-0 flex-col gap-0.5 max-md:col-start-1">
          <span class={[!off && "text-fg"]}>{everyText(s)}</span>
          {#if s.option}
            <a href={settingHref(s.option)} class="text-xs text-accent hover:underline"
              >{off ? "Turn on in Settings" : "Change in Settings"}</a>
          {:else}
            <span class="text-xs">{s.note ?? "Fixed"}</span>
          {/if}
        </div>
        <div class="flex min-w-0 flex-col gap-0.5 text-xs max-md:col-start-1">
          {#if s.running}
            <span class="flex items-center gap-1.5 text-fg"><Spinner class="size-3" />Running</span>
          {:else if s.lastRunAt}
            <span class="flex flex-wrap items-center gap-1.5">
              <span
                >Last run <Time at={s.lastRunAt} />{#if s.lastEndedAt}
                  (took {tookText(s.lastRunAt, s.lastEndedAt)}){/if}</span>
              {#if s.lastEnding && s.lastEnding !== "succeeded"}
                <Badge tone={tone(s)}>{ENDING_LABELS[s.lastEnding]}</Badge>
              {/if}
            </span>
            {#if s.lastError}
              <Tooltip text={s.lastError} clipped>
                {#snippet child({ props })}
                  <span {...props} class="truncate text-danger">{s.lastError}</span>
                {/snippet}
              </Tooltip>
            {:else if s.lastSummary}
              <Tooltip text={s.lastSummary} clipped>
                {#snippet child({ props })}
                  <span {...props} class="truncate text-fg-muted">{s.lastSummary}</span>
                {/snippet}
              </Tooltip>
            {/if}
            {#if s.lastJobId}
              <button
                type="button"
                class="w-fit text-accent hover:underline"
                onclick={() => onshow(s.lastJobId!)}>Show job</button>
            {/if}
          {:else}
            <span>Not run yet</span>
          {/if}
          {#if s.nextRunAt && !s.running}
            <span class="text-fg-muted">Next run <Time at={s.nextRunAt} /></span>
          {/if}
        </div>
        <div class="row-start-1 flex justify-end max-md:col-start-2 md:row-start-auto">
          {#if write && s.runnable}
            <Button
              size="sm"
              variant="outline"
              disabled={busy.running || s.running}
              onclick={() => run(s)}><PlayIcon />Run now</Button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>
</Card>

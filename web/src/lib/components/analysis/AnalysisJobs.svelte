<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";

  // the files waiting for or under analysis, with how far each is and a way to cancel it. The
  // toolbar this opens from keeps the queue current
  const write = $derived(session.can("ReadWrite"));
  const name = (path: string) => path.split(/[/\\]/).pop() ?? path;
  const cancel = async (id: string) => {
    const outcome = await jobs.cancel(id);
    if (!outcome.ok) toast(`Could not cancel the analysis: ${outcome.error}`, "error");
  };
  const waiting = $derived(jobs.list.filter((j) => j.state !== "running").length + jobs.backlog);
  const cancelWaiting = async () => {
    const outcome = await jobs.cancelWaiting();
    if (!outcome.ok) toast(`Could not cancel the waiting files: ${outcome.error}`, "error");
  };
</script>

<section class="flex flex-col gap-1">
  <div class="flex items-center gap-2">
    <h3 class="flex-1 text-sm font-medium">Analysis queue</h3>
    <!-- each listed file has its own cancel; the backlog has only this -->
    {#if write && (waiting > 1 || jobs.backlog)}
      <Button variant="ghost" size="sm" onclick={() => void cancelWaiting()}
        >Cancel {plural(waiting, "waiting file", "waiting files")}</Button>
    {/if}
  </div>
  {#if jobs.list.length}
    <ul class="flex flex-col gap-1">
      {#each jobs.list as job (job.id)}
        <li class="flex items-center gap-2 text-sm">
          <Tooltip text={job.path} dense>
            {#snippet child({ props })}
              <span {...props} class="min-w-0 flex-1 truncate">{name(job.path)}</span>
            {/snippet}
          </Tooltip>
          <span class="w-24 shrink-0 text-right text-xs text-fg-muted tabular-nums">
            {job.state === "running" ? `${Math.round((job.progress ?? 0) * 100)}%` : "Waiting"}
          </span>
          <Progress
            value={job.state === "running" ? (job.progress ?? 0) * 100 : null}
            label={`Analysis of ${name(job.path)}`}
            class="w-20 shrink-0 max-sm:hidden" />
          {#if write}
            <Tooltip text={`Cancel the analysis of ${name(job.path)}`}>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Cancel the analysis of ${name(job.path)}`}
                  onclick={() => void cancel(job.id)}><XIcon /></Button>
              {/snippet}
            </Tooltip>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if jobs.backlog}
    <p class="text-xs text-fg-muted tabular-nums">
      {plural(jobs.backlog, "more file joins", "more files join")} the queue as it empties
    </p>
  {:else if !jobs.list.length}
    <p class="text-sm text-fg-muted">Nothing in the queue</p>
  {/if}
</section>

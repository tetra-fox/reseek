<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { session } from "#lib/session.svelte.js";

  // the files waiting for or under analysis, with how far each is and a way to cancel it. The
  // toolbar this opens from keeps the queue current
  const write = $derived(session.can("ReadWrite"));
  const name = (path: string) => path.split(/[/\\]/).pop() ?? path;
  const cancel = async (id: string) => {
    const outcome = await jobs.cancel(id);
    if (!outcome.ok) toast(`Could not cancel the analysis: ${outcome.error}`, "error");
  };
</script>

<section class="flex flex-col gap-1">
  <h3 class="text-sm font-medium">Analysis queue</h3>
  {#if jobs.list.length}
    <ul class="flex flex-col gap-1">
      {#each jobs.list as job (job.id)}
        <li class="flex items-center gap-2 text-sm">
          <span class="min-w-0 flex-1 truncate" title={job.path}>{name(job.path)}</span>
          <span class="w-24 shrink-0 text-right text-xs text-fg-muted tabular-nums">
            {job.state === "running" ? `${Math.round((job.progress ?? 0) * 100)}%` : "Waiting"}
          </span>
          <div
            class="h-1.5 w-20 shrink-0 overflow-hidden rounded-full bg-surface-muted max-sm:hidden">
            <div class="h-full bg-accent" style:width={`${(job.progress ?? 0) * 100}%`}></div>
          </div>
          {#if write}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Cancel the analysis of ${name(job.path)}`}
              onclick={() => void cancel(job.id)}><XIcon /></Button>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-sm text-fg-muted">No files are waiting or being analyzed.</p>
  {/if}
</section>

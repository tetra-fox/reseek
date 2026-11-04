<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import type { Job } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { session } from "#lib/session.svelte.js";

  // how far a file's analysis is in the queue, with a way to cancel it
  let { job }: { job: Job } = $props();

  const write = $derived(session.can("ReadWrite"));
  const running = $derived(job.state === "running");
  const percent = $derived(Math.round((job.progress ?? 0) * 100));
</script>

<div class="flex items-center gap-2 text-sm">
  <div class="flex flex-1 flex-col gap-1">
    <span>{running ? `Analyzing: ${percent}%` : "Waiting in the analysis queue"}</span>
    <!-- a queued job has no progress yet, which the bar says by leaving its value out -->
    <div
      class="h-1.5 overflow-hidden rounded-full bg-surface-muted"
      role="progressbar"
      aria-label="Analysis progress"
      aria-valuenow={running ? percent : undefined}
      aria-valuemin={0}
      aria-valuemax={100}>
      <div
        class="h-full bg-accent transition-[width]"
        style:width={`${(job.progress ?? 0) * 100}%`}>
      </div>
    </div>
  </div>
  {#if write}
    <Button variant="outline" size="sm" onclick={() => void jobs.cancel(job.id)}
      ><XIcon />Cancel</Button>
  {/if}
</div>

<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import type { Job } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
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
    <Progress value={running ? percent : null} label="Analysis progress" />
  </div>
  {#if write}
    <Button variant="outline" size="sm" onclick={() => void jobs.cancel(job.id)}
      ><XIcon />Cancel</Button>
  {/if}
</div>

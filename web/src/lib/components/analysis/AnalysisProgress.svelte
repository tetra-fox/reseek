<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import type { Job } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { session } from "#lib/session.svelte.js";

  // how far a file's analysis is in the queue, with a way to cancel it
  let { job }: { job: Job } = $props();

  const write = $derived(session.can("ReadWrite"));
</script>

<div class="flex items-center gap-2 text-sm" role="status">
  <div class="flex flex-1 flex-col gap-1">
    <span>
      {job.state === "running"
        ? `Analyzing: ${Math.round((job.progress ?? 0) * 100)}%`
        : "Waiting in the analysis queue"}
    </span>
    <div class="h-1.5 overflow-hidden rounded-full bg-surface-muted">
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

<script lang="ts">
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import XIcon from "@lucide/svelte/icons/x";

  import type { SlotId } from "#lib/analysis/address.js";
  import type { FileReport } from "#lib/analysis/report.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import { trackOf } from "#lib/analysis/track.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { getFileName } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";

  import AnalysisProgress from "./AnalysisProgress.svelte";
  import EmptySlot from "./EmptySlot.svelte";
  import Judgment from "./Judgment.svelte";
  import Reading from "./Reading.svelte";
  import SpectrogramView from "./SpectrogramView.svelte";

  // one of the page's two places for a file: empty, receiving a file, or showing one with its
  // judgment and reading beside it. All of it is a drop area, which a filled slot is too
  type Props = {
    id: SlotId;
    analyzed: FileReport;
    /** whether an empty slot is a slim bar, with the other slot in use */
    slim: boolean;
    /** the plots' heights in rem, smaller with two files to show */
    heights: { plot: number; wave: number };
  };

  let { id, analyzed, slim, heights }: Props = $props();

  const letter = $derived(id.toUpperCase());
  const loaded = $derived(slots[id]);
  const sending = $derived(slots.sending[id]);
  const local = $derived(slots.local(id));
  const report = $derived(analyzed.report);
  const scope = $derived(analyzed.scope);
  const job = $derived(analyzed.job);
  const write = $derived(session.can("ReadWrite"));
  const path = $derived(
    report?.relativePath ??
      job?.relativePath ??
      (loaded?.target.kind === "file" ? loaded.target.path : null)
  );
  const name = $derived(loaded?.name ?? (path && getFileName(path)));
  const track = $derived(loaded && report ? trackOf(loaded.target, report.relativePath) : null);

  /** a file dragged over the slot, which a drag across its children keeps counted */
  let depth = 0;
  let over = $state(false);
  const carriesFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes("Files");
  const entered = (e: DragEvent) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    depth++;
    over = true;
  };
  const moved = (e: DragEvent) => {
    if (carriesFiles(e)) e.preventDefault();
  };
  const left = () => {
    depth = Math.max(0, depth - 1);
    over = depth > 0;
  };
  const dropped = (e: DragEvent) => {
    e.preventDefault();
    depth = 0;
    over = false;
    const [file] = e.dataTransfer?.files ?? [];
    if (!file) return;
    if (e.dataTransfer?.items[0]?.webkitGetAsEntry()?.isDirectory) {
      return void toast("A folder cannot be analyzed. Drop a single file.", "error");
    }
    if (!write) return void toast("Uploading a file requires write access.", "error");
    slots.upload(id, file);
  };
</script>

<section
  aria-label={`Slot ${letter}`}
  class={[
    "rounded-card border bg-surface shadow-card transition-shadow duration-(--motion-fast)",
    over && "ring-2 ring-accent"
  ]}
  ondragenter={entered}
  ondragover={moved}
  ondragleave={left}
  ondrop={dropped}>
  {#if sending}
    <div class="flex items-center gap-3 p-3 text-sm" role="status">
      <span class="font-medium">{letter}</span>
      <div class="flex min-w-0 flex-1 flex-col gap-1">
        <span class="truncate">Uploading {sending.name}: {Math.round(sending.sent * 100)}%</span>
        <div class="h-1.5 overflow-hidden rounded-full bg-surface-muted">
          <div class="h-full bg-accent transition-[width]" style:width={`${sending.sent * 100}%`}>
          </div>
        </div>
      </div>
      <Button variant="outline" size="sm" onclick={() => slots.cancel(id)}><XIcon />Cancel</Button>
    </div>
  {:else if !loaded}
    <EmptySlot {id} {slim} />
  {:else}
    <header class="flex min-h-11 items-center gap-2 border-b px-3 py-1.5">
      <span class="font-medium">{letter}</span>
      {#if name || report === undefined}
        {#if name}
          <h2 class="min-w-0 truncate text-sm font-medium" title={local ? name : (path ?? name)}>
            {name}
          </h2>
        {:else}
          <Skeleton class="h-4 w-48" />
        {/if}
      {:else}
        <h2 class="text-sm font-medium">A finished download</h2>
      {/if}
      {#if local}
        <Badge tone="info" class="shrink-0">Local file</Badge>
      {/if}
      <Button
        variant="ghost"
        size="sm"
        class="ml-auto shrink-0"
        aria-label={`Clear slot ${letter}`}
        onclick={() => slots.clear(id)}><XIcon />Clear</Button>
    </header>
    <div class="flex flex-col gap-3 p-3">
      {#if job}<AnalysisProgress {job} />{/if}
      {#if analyzed.error}
        <p class="text-sm text-fg-muted">{analyzed.error}</p>
      {:else if report === undefined}
        <Skeleton class="h-40" />
      {:else if report === null}
        {#if !job}
          <p class="text-sm text-fg-muted">This file has not been analyzed.</p>
          {#if write}
            <Button
              variant="outline"
              size="sm"
              class="self-start"
              onclick={() => void analyzed.analyze(false)}
              ><A.analyze.icon />{A.analyze.label}</Button>
          {/if}
        {/if}
      {:else if report.stale}
        {#if !job}
          <p class="text-sm text-fg-muted">
            This file was analyzed by an older version of reseek. Analyze it again to view the
            results.
          </p>
          {#if write}
            <Button
              variant="outline"
              size="sm"
              class="self-start"
              onclick={() => void analyzed.analyze(true)}><RefreshCwIcon />Analyze again</Button>
          {/if}
        {/if}
        {#if !local}
          <Judgment target={loaded.target} {report} onsaved={() => void analyzed.load()} />
        {/if}
      {:else}
        {#snippet beside()}
          {#if !local}
            <Judgment target={loaded.target} {report} onsaved={() => void analyzed.load()} />
          {/if}
          <Reading {report} />
          {#if write && !job}
            <Button
              variant="ghost"
              size="sm"
              class="self-start"
              onclick={() => void analyzed.analyze(true)}><RefreshCwIcon />Analyze again</Button>
          {/if}
        {/snippet}
        {#if scope}
          <!-- a file or a new report starts the viewer over, and stops its sound -->
          {#key scope}
            <SpectrogramView {scope} {track} {heights} aside={beside} />
          {/key}
        {:else}
          <p class="text-sm">{report.summary}</p>
          {@render beside()}
        {/if}
      {/if}
    </div>
  {/if}
</section>

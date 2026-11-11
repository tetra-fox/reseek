<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import XIcon from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";

  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import type { Enqueued } from "#lib/files/downloads.js";
  import { formatBytes, plural } from "#lib/format.js";
  import { footBars } from "#lib/ui/footbars.svelte.js";

  type Props = {
    /** the selected files: how many, and their sizes */
    files: { size?: number | null }[];
    /** downloads them, when they can be downloaded now; what it came to shows beside the button */
    ondownload?: () => Promise<Enqueued>;
    onclear: () => void;
    /** the screen's own actions on the selection */
    actions?: Snippet;
    /** what the selection is of, one and many: files unless said */
    noun?: [string, string];
  };

  let { files, ondownload, onclear, actions, noun = ["file", "files"] }: Props = $props();

  let busy = $state(false);
  let status = $state<Enqueued | null>(null);
  const bytes = $derived(files.reduce((n, f) => n + (f.size ?? 0), 0));

  const download = async () => {
    if (!ondownload) return;
    busy = true;
    status = null;
    status = await ondownload();
    busy = false;
  };
</script>

<!-- what is selected in a list of files and what can be done with it, at the foot of the list -->
<div class="selection-bar" role="toolbar" aria-label="Selected files" {@attach footBars.attach}>
  <span class="text-sm tabular-nums">
    {plural(files.length, noun[0], noun[1])}{bytes ? `, ${formatBytes(bytes)}` : ""}
  </span>
  {#if ondownload}
    <Button size="sm" disabled={busy} onclick={() => void download()}>
      <DownloadIcon />Download
    </Button>
  {/if}
  {@render actions?.()}
  {#if busy}
    <Spinner class="size-4" label="Queueing" />
  {:else if status?.kind === "done"}
    <CheckIcon class="size-4 text-success" aria-label={status.text} />
  {:else if status}
    <span class="flex items-center gap-1 text-sm text-danger">
      <XIcon class="size-4" />{status.text}
    </span>
  {/if}
  <Button variant="ghost" size="sm" onclick={onclear}>Clear</Button>
</div>

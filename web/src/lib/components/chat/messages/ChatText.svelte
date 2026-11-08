<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";

  import { richText, slskText } from "#lib/chat/messages/rich.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { enqueue } from "#lib/files/downloads.js";
  import { slskBrowse, type SlskLink } from "#lib/files/slsk.js";
  import { session } from "#lib/session.svelte.js";

  // a chat message's text, shown as text: web links open in a new tab, slsk:// links open the
  // folder on the user's Shares tab with a download button for a file, and the watched words are
  // marked
  let { text, watch }: { text: string; watch: string[] } = $props();

  const parts = $derived(richText(text, watch));
  const write = $derived(session.can("ReadWrite"));

  const download = async (link: SlskLink) => {
    // the size is not in the link; the daemon learns it from the peer
    const done = await enqueue({
      username: link.username,
      files: [{ filename: link.path, size: 0 }]
    });
    toast(done.text, done.kind === "done" ? "info" : "error");
  };
</script>

{#each parts as part (part.at)}{#if part.kind === "url"}<a
      href={part.url}
      target="_blank"
      rel="noopener noreferrer"
      class="text-accent underline-offset-2 hover:underline">{part.text}</a
    >{:else if part.kind === "slsk"}<Tooltip
      text={`${part.link.folder ? "Browse this folder" : "Show this file"} in ${part.link.username}'s shares`}
      >{#snippet child({ props })}<a
          {...props}
          href={slskBrowse(part.link)}
          class="text-accent underline-offset-2 hover:underline">{slskText(part.link)}</a
        >{/snippet}</Tooltip
    >{#if write && !part.link.folder}<Tooltip text="Download"
        >{#snippet child({ props })}<button
            {...props}
            type="button"
            class="mx-0.5 inline-flex translate-y-0.5 items-center rounded-badge p-0.5 text-fg-muted hover:bg-hover hover:text-fg"
            aria-label={`Download ${part.link.path}`}
            onclick={() => void download(part.link)}><DownloadIcon class="size-3.5" /></button
          >{/snippet}</Tooltip
      >{/if}{:else if part.kind === "mark"}<mark
      class="rounded-[3px] bg-warning-soft px-0.5 text-warning-soft-fg">{part.text}</mark
    >{:else}{part.text}{/if}{/each}

<script lang="ts">
  import { goto } from "$app/navigation";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LinkIcon from "@lucide/svelte/icons/link";
  import ListIcon from "@lucide/svelte/icons/list";
  import PlayIcon from "@lucide/svelte/icons/play";
  import SaveIcon from "@lucide/svelte/icons/save";
  import type { Snippet } from "svelte";

  import { copyText } from "#lib/clipboard.svelte.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { slskLink } from "#lib/slsk.js";

  import { isAudio, remoteFolder } from "./links";

  type Props = {
    /** the file's path as it is shared, and whose share it is in */
    path: string;
    username?: string;
    /** enqueues the file, where a row can be downloaded */
    ondownload?: () => void;
    /** for a file on this machine: its inline and attachment URLs */
    local?: { open: string; download: string };
    /** plays the file in the page's player, for audio on this machine */
    onpreview?: () => void;
    /** the element that opens the menu, which spreads `props` */
    trigger: Snippet<[{ props: Record<string, unknown> }]>;
  };

  let { path, username, ondownload, local, onpreview, trigger }: Props = $props();

  const folder = $derived(username ? remoteFolder(username, path) : null);
</script>

<!-- the actions every file row offers, wherever the file is listed -->
<Menu.Root>
  <Menu.Trigger>
    {#snippet child({ props })}{@render trigger({ props })}{/snippet}
  </Menu.Trigger>
  <Menu.Content align="end" class="min-w-48">
    {#if ondownload}
      <Menu.Item onSelect={ondownload}><DownloadIcon />Download</Menu.Item>
    {/if}
    {#if folder}
      <Menu.Item onSelect={() => goto(folder.flat)}><ListIcon />View folder</Menu.Item>
      <Menu.Item onSelect={() => goto(folder.browse)}><FolderOpenIcon />Browse folder</Menu.Item>
    {/if}
    <Menu.Item onSelect={() => void copyText(path)}><CopyIcon />Copy path</Menu.Item>
    {#if username}
      <Menu.Item onSelect={() => void copyText(slskLink(username, path))}
        ><LinkIcon />Copy slsk:// link</Menu.Item>
    {/if}
    {#if local}
      <Menu.Separator />
      <Menu.Item onSelect={() => window.open(local.open, "_blank", "noreferrer")}>
        <ExternalLinkIcon />Open
      </Menu.Item>
      {#if onpreview && isAudio(path)}
        <Menu.Item onSelect={onpreview}><PlayIcon />Preview</Menu.Item>
      {/if}
      <Menu.Item onSelect={() => window.location.assign(local.download)}>
        <SaveIcon />Download to device
      </Menu.Item>
    {/if}
  </Menu.Content>
</Menu.Root>

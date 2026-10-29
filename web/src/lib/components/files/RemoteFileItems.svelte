<script lang="ts">
  import { goto } from "$app/navigation";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { remoteFolder } from "#lib/files/links.js";
  import { slskLink } from "#lib/files/slsk.js";

  type Props = {
    /** the file's path as it is shared, and whose share it is in */
    path: string;
    username: string;
    /** enqueues the file, where it can be downloaded now */
    ondownload?: () => void;
  };

  let { path, username, ondownload }: Props = $props();
</script>

<!-- a file in someone's share: download it, go to its folder, copy where it is -->
{#if ondownload}
  <Menu.Item onSelect={ondownload}><A.download.icon />{A.download.label}</Menu.Item>
{/if}
<Menu.Item onSelect={() => goto(remoteFolder(username, path).browse)}
  ><A.browseFolder.icon />{A.browseFolder.label}</Menu.Item>
<Menu.Item onSelect={() => void copyText(path)}><A.copyPath.icon />{A.copyPath.label}</Menu.Item>
<Menu.Item onSelect={() => void copyText(slskLink(username, path))}
  ><A.copyLink.icon />{A.copyLink.label}</Menu.Item>

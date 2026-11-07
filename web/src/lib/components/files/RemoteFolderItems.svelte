<script lang="ts">
  import { goto } from "$app/navigation";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { folderLinks } from "#lib/files/links.js";
  import { slskLink } from "#lib/files/slsk.js";

  type Props = {
    /** the folder's path as it is shared, and whose share it is in */
    folder: string;
    username: string;
    /** enqueues every file under it, where they can be downloaded now */
    ondownload?: () => void;
    /** lists every file under it as its user has them, where the screen can */
    onallfiles?: () => void;
    /** a Browse item, which the user's own shares screen has no use for */
    browse?: boolean;
  };

  let { folder, username, ondownload, onallfiles, browse = true }: Props = $props();
</script>

<!-- a folder in someone's share: download it, list or browse it, copy where it is -->
{#if ondownload}
  <Menu.Item onSelect={ondownload}><A.downloadFolder.icon />{A.downloadFolder.label}</Menu.Item>
{/if}
{#if onallfiles}
  <Menu.Item onSelect={onallfiles}><A.allFiles.icon />{A.allFiles.label}</Menu.Item>
{/if}
{#if browse}
  <Menu.Item onSelect={() => goto(folderLinks(username, folder).browse)}
    ><A.browseFolder.icon />{A.browseFolder.label}</Menu.Item>
{/if}
<Menu.Item onSelect={() => void copyText(folder)}><A.copyPath.icon />{A.copyPath.label}</Menu.Item>
<Menu.Item onSelect={() => void copyText(slskLink(username, folder, true))}
  ><A.copyLink.icon />{A.copyLink.label}</Menu.Item>

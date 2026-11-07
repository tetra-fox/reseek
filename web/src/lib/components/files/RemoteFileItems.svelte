<script lang="ts">
  import { goto } from "$app/navigation";
  import InfoIcon from "@lucide/svelte/icons/info";
  import SendIcon from "@lucide/svelte/icons/send";

  import { app } from "#lib/app.svelte.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { remoteFolder } from "#lib/files/links.js";
  import { slskLink } from "#lib/files/slsk.js";
  import { session } from "#lib/session.svelte.js";
  import { sending } from "#lib/transfers/send.svelte.js";

  type Props = {
    /** the file's path as it is shared, and whose share it is in */
    path: string;
    username: string;
    /** enqueues the file, where it can be downloaded now */
    ondownload?: () => void;
    /** opens the inspector on the file; without it there is no Details */
    ondetails?: () => void;
  };

  let { path, username, ondownload, ondetails }: Props = $props();
  // a file of our own shares can be sent to another user
  const ours = $derived(username === app.state?.user.username && session.can("ReadWrite"));
</script>

<!-- a file in someone's share: download it, go to its folder, copy where it is, see its details -->
{#if ondownload}
  <Menu.Item onSelect={ondownload}><A.download.icon />{A.download.label}</Menu.Item>
{/if}
<Menu.Item onSelect={() => goto(remoteFolder(username, path).browse)}
  ><A.browseFolder.icon />{A.browseFolder.label}</Menu.Item>
<Menu.Item onSelect={() => void copyText(path)}><A.copyPath.icon />{A.copyPath.label}</Menu.Item>
<Menu.Item onSelect={() => void copyText(slskLink(username, path))}
  ><A.copyLink.icon />{A.copyLink.label}</Menu.Item>
{#if ours}
  <Menu.Item onSelect={() => sending.ask([path])}><SendIcon />Send to a user...</Menu.Item>
{/if}
{#if ondetails}
  <Menu.Separator />
  <Menu.Item onSelect={ondetails}><InfoIcon />Details</Menu.Item>
{/if}

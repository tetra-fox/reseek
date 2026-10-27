<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronsUpIcon from "@lucide/svelte/icons/chevrons-up";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import InfoIcon from "@lucide/svelte/icons/info";
  import ListIcon from "@lucide/svelte/icons/list";
  import ListOrderedIcon from "@lucide/svelte/icons/list-ordered";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import SaveIcon from "@lucide/svelte/icons/save";
  import SearchIcon from "@lucide/svelte/icons/search";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import UserIcon from "@lucide/svelte/icons/user";
  import XIcon from "@lucide/svelte/icons/x";

  import { leaf } from "#lib/browse/tree.js";
  import { isAudio, remoteFolder } from "#lib/components/files/links.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { links } from "#lib/components/users/actions.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import {
    act,
    bumpUpload,
    copyPath,
    open,
    openFolder,
    preview,
    queuePosition,
    redownload,
    saveToDevice,
    storeOf
  } from "#lib/transfers/actions.js";
  import { type Direction, rowActions, type Transfer } from "#lib/transfers/states.js";

  type Props = {
    transfer: Transfer;
    direction: Direction;
    /** the transfers of its folder, which the player plays around it */
    around: Transfer[];
    /** removes it, asking first when it is still running */
    onremove: () => void;
    /** blocks the user, asking first; with `deny` their unfinished uploads are cancelled first */
    onblock: (deny: boolean) => void;
    /** opens the details drawer; without it the menu has no Details */
    ondetails?: () => void;
  };

  let { transfer: t, direction, around, onremove, onblock, ondetails }: Props = $props();

  const can = $derived(rowActions(t, direction));
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const folder = $derived(remoteFolder(t.username, t.filename));
  const to = $derived(links(t.username, leaf(folder.folder)));
</script>

<!-- the whole row's actions; placed in a Menu.Root by the caller -->
<Menu.Content align="end" class="min-w-56">
  {#if can.open}
    <Menu.Item onSelect={() => void open(t)}><ExternalLinkIcon />Open</Menu.Item>
    {#if isAudio(t.filename)}
      <Menu.Item onSelect={() => preview(t, around)}><PlayIcon />Preview</Menu.Item>
    {/if}
    <Menu.Item onSelect={() => void saveToDevice(t)}><SaveIcon />Download to device</Menu.Item>
    <Menu.Item onSelect={() => openFolder([t])}><FolderOpenIcon />Open containing folder</Menu.Item>
  {/if}
  {#if t.localPath}
    <Menu.Item onSelect={() => copyPath(t.localPath!)}><CopyIcon />Copy local path</Menu.Item>
  {/if}
  <Menu.Item onSelect={() => copyPath(t.filename)}><CopyIcon />Copy remote path</Menu.Item>
  <Menu.Separator />
  <Menu.Item onSelect={() => goto(folder.browse)}>
    <FolderOpenIcon />Browse this folder on {t.username}
  </Menu.Item>
  <Menu.Item onSelect={() => goto(folder.flat)}
    ><ListIcon />View all files in this folder</Menu.Item>
  {#if write}
    <Menu.Item onSelect={() => goto(to.search)}><SearchIcon />More from this user</Menu.Item>
    <Menu.Separator />
    {#if can.retry}
      <Menu.Item onSelect={() => void act(storeOf(t), "retry", [t.id])}>
        <RotateCwIcon />Retry
      </Menu.Item>
    {/if}
    {#if can.cancel}
      <Menu.Item onSelect={() => void act(storeOf(t), "cancel", [t.id])}><XIcon />Cancel</Menu.Item>
    {/if}
    <Menu.Item onSelect={onremove}><Trash2Icon />Remove</Menu.Item>
    {#if can.redownload}
      <Menu.Item onSelect={() => void redownload(t)}><RefreshCwIcon />Re-download</Menu.Item>
    {/if}
  {/if}
  {#if can.queuePosition}
    <Menu.Item onSelect={() => void queuePosition(t)}><ListOrderedIcon />Queue position</Menu.Item>
  {/if}
  {#if can.bump && admin}
    <Menu.Item onSelect={() => void bumpUpload(t)}><ChevronsUpIcon />Bump</Menu.Item>
  {/if}
  <Menu.Separator />
  {#if write}
    <Menu.Item onSelect={() => goto(to.message)}>
      <MessageSquareIcon />Message {t.username}
    </Menu.Item>
  {/if}
  <Menu.Item onSelect={() => goto(userHref(t.username))}><UserIcon />View profile</Menu.Item>
  {#if write}
    <Menu.Item onSelect={() => onblock(false)}><BanIcon />Block {t.username}</Menu.Item>
    {#if can.deny}
      <Menu.Item onSelect={() => onblock(true)}><BanIcon />Deny and block</Menu.Item>
    {/if}
  {/if}
  {#if ondetails}
    <Menu.Separator />
    <Menu.Item onSelect={ondetails}><InfoIcon />Details</Menu.Item>
  {/if}
</Menu.Content>

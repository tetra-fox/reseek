<script lang="ts">
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronsUpIcon from "@lucide/svelte/icons/chevrons-up";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import InfoIcon from "@lucide/svelte/icons/info";
  import ListOrderedIcon from "@lucide/svelte/icons/list-ordered";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import LocalFileItems from "#lib/components/files/LocalFileItems.svelte";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import UserMenuItems from "#lib/components/users/UserMenuItems.svelte";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { remoteFolder } from "#lib/files/links.js";
  import { getFileName } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import {
    act,
    bumpUpload,
    copyPath,
    open,
    openFolder,
    queuePosition,
    redownload,
    saveToDevice,
    storeOf
  } from "#lib/transfers/actions.js";
  import { type Direction, rowActions, type Transfer } from "#lib/transfers/states.js";
  import { shiftHeld } from "#lib/ui/keys.js";

  type Props = {
    transfer: Transfer;
    direction: Direction;
    /** removes it, asking first when it is still running */
    onremove: () => void;
    /** blocks the user, asking first; with `deny` their unfinished uploads are canceled first */
    onblock: (deny: boolean) => void;
    /** opens the inspector on the file; without it the menu has no Details */
    ondetails?: () => void;
    /** opens the file's analysis, for a file the analyzer takes; without it the menu has no Analyze */
    onanalyze?: (large: boolean) => void;
  };

  let { transfer: t, direction, onremove, onblock, ondetails, onanalyze }: Props = $props();

  const can = $derived(rowActions(t, direction));
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const folder = $derived(remoteFolder(t.username, t.filename));
</script>

<!-- the whole row's actions; placed in a Menu.Root or Menu.LazyRoot by the caller -->
<Menu.Content align="end" class="min-w-56">
  {#if can.open}
    <LocalFileItems
      onopen={() => void open(t)}
      onsave={() => void saveToDevice(t)}
      path={t.localPath}
      local />
    <Menu.Item onSelect={() => openFolder([t])}><FolderOpenIcon />Open containing folder</Menu.Item>
  {:else if t.localPath}
    <Menu.Item onSelect={() => copyPath(t.localPath!)}
      ><A.copyLocalPath.icon />{A.copyLocalPath.label}</Menu.Item>
  {/if}
  <Menu.Separator />
  <RemoteFileItems path={t.filename} username={t.username} />
  {#if write}
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
    <Menu.Item onSelect={() => void queuePosition(t)}
      ><ListOrderedIcon />Check queue position</Menu.Item>
  {/if}
  {#if can.bump && admin}
    <Menu.Item onSelect={() => void bumpUpload(t)}
      ><ChevronsUpIcon />Move to front of queue</Menu.Item>
  {/if}
  <Menu.Separator />
  <UserMenuItems name={t.username} folder={getFileName(folder.folder)} named />
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
  {#if onanalyze}
    {#if !ondetails}<Menu.Separator />{/if}
    <Menu.Item onSelect={() => onanalyze(shiftHeld())}><AudioLinesIcon />Analyze</Menu.Item>
  {/if}
</Menu.Content>

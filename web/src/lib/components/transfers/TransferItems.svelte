<script lang="ts">
  import ArrowDownToLineIcon from "@lucide/svelte/icons/arrow-down-to-line";
  import ArrowUpToLineIcon from "@lucide/svelte/icons/arrow-up-to-line";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronsUpIcon from "@lucide/svelte/icons/chevrons-up";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import InfoIcon from "@lucide/svelte/icons/info";
  import ListOrderedIcon from "@lucide/svelte/icons/list-ordered";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import type { Target } from "#lib/analysis/analysis.js";
  import AnalysisItems from "#lib/components/analysis/AnalysisItems.svelte";
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
    saveToDevice,
    storeOf
  } from "#lib/transfers/actions.js";
  import { type Direction, rowActions, type Transfer } from "#lib/transfers/states.js";
  import { downloads } from "#lib/transfers/transfers.svelte.js";

  type Props = {
    transfer: Transfer;
    direction: Direction;
    /** removes it, asking first when it is still running */
    onremove: () => void;
    /** blocks the user, asking first; with `deny` their unfinished uploads are canceled first */
    onblock: (deny: boolean) => void;
    /** opens the inspector on the file; without it the menu has no Details */
    ondetails?: () => void;
    /** the file as the analyzer names it, for a file the analyzer takes */
    analysis?: { target: Target; name: string } | null;
  };

  let { transfer: t, direction, onremove, onblock, ondetails, analysis }: Props = $props();

  const can = $derived(rowActions(t, direction));
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const folder = $derived(remoteFolder(t.username, t.filename));
  /** the download waiting first in the local queue, which Start next goes before */
  const firstWaiting = () =>
    [...downloads.places.entries()].find(([, place]) => place === 1)?.[0] ?? null;
</script>

<!-- everything a transfer's row offers, as menu items: its ... button, a right-click on the row and
     the inspector all show these -->
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
  <!-- both go through the daemon's retry, which keeps the download's folder and search -->
  {#if can.retry}
    <Menu.Item onSelect={() => void act(storeOf(t), "retry", [t.id])}>
      <RotateCwIcon />Retry
    </Menu.Item>
  {:else if can.downloadAgain}
    <Menu.Item onSelect={() => void act(storeOf(t), "retry", [t.id])}>
      <RefreshCwIcon />Download again
    </Menu.Item>
  {/if}
  {#if can.pause}
    <Menu.Item onSelect={() => void act(storeOf(t), "pause", [t.id])}><PauseIcon />Pause</Menu.Item>
  {:else if can.resume}
    <Menu.Item onSelect={() => void act(storeOf(t), "resume", [t.id])}
      ><PlayIcon />Resume</Menu.Item>
  {/if}
  {#if can.reorder}
    <Menu.Item onSelect={() => void downloads.move([t.id], firstWaiting())}
      ><ArrowUpToLineIcon />Start next</Menu.Item>
    <Menu.Item onSelect={() => void downloads.move([t.id], null)}
      ><ArrowDownToLineIcon />Start last</Menu.Item>
  {/if}
  {#if can.cancel}
    <Menu.Item onSelect={() => void act(storeOf(t), "cancel", [t.id])}><XIcon />Cancel</Menu.Item>
  {/if}
  <Menu.Item onSelect={onremove}><Trash2Icon />Remove</Menu.Item>
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
{#if analysis}
  {#if !ondetails}<Menu.Separator />{/if}
  <AnalysisItems {...analysis} />
{/if}

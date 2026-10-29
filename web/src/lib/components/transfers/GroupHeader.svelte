<script lang="ts">
  import { goto } from "$app/navigation";
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
  import BanIcon from "@lucide/svelte/icons/ban";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FileArchiveIcon from "@lucide/svelte/icons/file-archive";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import FolderSearchIcon from "@lucide/svelte/icons/folder-search";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import UserXIcon from "@lucide/svelte/icons/user-x";
  import XIcon from "@lucide/svelte/icons/x";
  import type { Component } from "svelte";

  import { analyzableDownload, analyzeBatch, downloadCandidate } from "#lib/analysis/batch.js";
  import { analyzerOffered } from "#lib/analysis/marks.svelte.js";
  import { root } from "#lib/api/client.js";
  import { folderLinks, remoteFolder } from "#lib/components/files/links.js";
  import TreeToggle from "#lib/components/tree/TreeToggle.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Username from "#lib/components/users/Username.svelte";
  import { formatBytes, formatDuration, plural } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { act, folderOf, openFolder } from "#lib/transfers/actions.js";
  import { encodePath } from "#lib/transfers/content.js";
  import { groupProgress } from "#lib/transfers/progress.js";
  import { isCancelable, isRetryable, isSucceeded, type Direction } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Group } from "#lib/transfers/view.js";
  import { quota, waits } from "#lib/transfers/why.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { GAP_REM, lineStart } from "#lib/tree/indent.js";
  import { viewport } from "#lib/viewport.svelte.js";

  type Props = {
    group: Group;
    store: Transfers;
    direction: Direction;
    /** which groups are open */
    expansion: Expansion;
    ticked: number;
    onselect: (on: boolean) => void;
    /** removes the group's transfers, asking first for many or running ones */
    onremove: () => void;
    /** cancels the group's transfers, asking first when any is moving bytes */
    oncancel: () => void;
    /** blocks the user, asking first; with `deny` their unfinished uploads are canceled first */
    onblock: (deny: boolean) => void;
  };

  let { group, store, direction, expansion, ticked, onselect, onremove, oncancel, onblock }: Props =
    $props();

  const write = $derived(session.can("ReadWrite"));
  const list = $derived(group.transfers);
  const bytes = $derived(list.reduce((n, t) => n + t.size, 0));
  const done = $derived(list.filter((t) => t.state.includes("Completed")).length);
  const failed = $derived(list.filter((t) => isRetryable(t.state)).length);
  const progress = $derived(groupProgress(list));
  const folded = $derived(!expansion.isOpen(group.key));
  const local = $derived(direction === "download" ? folderOf(list) : null);
  // a user's quota, from why the first of their queued uploads waits
  const waiting = $derived(
    direction === "upload" && group.kind === "user"
      ? list.find((t) => t.state.includes("Queued"))
      : undefined
  );
  $effect(() => {
    if (waiting) void waits.load(waiting);
  });
  const usage = $derived(waiting && waits.get(waiting.id) ? quota(waits.get(waiting.id)!) : null);

  /** Saves the group's local folder as a zip; only finished files are in it */
  const zip = () => {
    if (!local) return;
    if (list.some((t) => !isSucceeded(t.state)))
      toast("Only finished files are included in the zip");
    const name = local.split("/").pop() || "downloads";
    window.location.assign(
      `${root()}/api/v0/files/downloads/directories/${encodePath(local)}/archive?name=${encodeURIComponent(name)}`
    );
  };

  const folderActions = () => direction === "download" && group.kind === "folder";
  const userUploads = () => write && direction === "upload" && group.kind === "user";

  /**
   * One of the group's actions: `shown` says whether it applies now, and `run` reads the group
   * when it is chosen. `text` shows its label beside the icon on a wide screen
   */
  type Action = {
    label: string;
    icon: Component;
    shown: () => boolean;
    run: () => void;
    text?: boolean;
  };

  const ACTIONS: Action[] = [
    {
      label: "Retry the group",
      icon: RotateCwIcon,
      shown: () => write && direction === "download" && list.some((t) => isRetryable(t.state)),
      run: () =>
        void act(
          store,
          "retry",
          list.filter((t) => isRetryable(t.state)).map((t) => t.id)
        )
    },
    {
      label: "Cancel the group",
      icon: XIcon,
      shown: () => write && list.some((t) => isCancelable(t.state)),
      run: () => oncancel()
    },
    { label: "Remove the group", icon: Trash2Icon, shown: () => write, run: () => onremove() },
    {
      label: "Download more from this folder",
      icon: FolderSearchIcon,
      shown: folderActions,
      run: () =>
        void goto(
          group.folder
            ? folderLinks(group.username, group.folder).browse
            : remoteFolder(group.username, list[0].filename).browse
        )
    },
    {
      label: "Open folder",
      icon: FolderOpenIcon,
      shown: () => folderActions() && local !== null,
      run: () => openFolder(list)
    },
    // files directly in the downloads directory have no folder to zip
    {
      label: "Download folder",
      icon: FileArchiveIcon,
      shown: () => folderActions() && !!local,
      run: zip
    },
    {
      get label() {
        return group.kind === "folder" ? "Analyze folder" : "Analyze the group";
      },
      icon: AudioLinesIcon,
      shown: () =>
        write &&
        direction === "download" &&
        group.kind !== "batch" &&
        analyzerOffered() &&
        list.some(analyzableDownload),
      run: () => void analyzeBatch(list.map(downloadCandidate), `${group.label} has no downloads`)
    },
    {
      label: "Deny queued uploads",
      icon: UserXIcon,
      shown: userUploads,
      run: () =>
        void act(
          store,
          "cancel",
          list.filter((t) => t.state.includes("Queued")).map((t) => t.id)
        ),
      text: true
    },
    {
      get label() {
        return `Block ${group.username}`;
      },
      icon: BanIcon,
      shown: userUploads,
      run: () => onblock(false)
    }
  ];
  const actions = $derived(ACTIONS.filter((a) => a.shown()));
</script>

<div
  class="flex h-full items-center bg-surface-muted/80 px-2 text-sm"
  style:padding-left={`${lineStart(group.depth)}rem`}
  style:column-gap={`${GAP_REM}rem`}
  role="rowheader">
  <Checkbox
    aria-label={`Select every transfer in ${group.label}`}
    checked={ticked > 0 && ticked === list.length}
    indeterminate={ticked > 0 && ticked < list.length}
    onCheckedChange={onselect} />
  <TreeToggle {expansion} key={group.key} siblings={group.siblings} name={group.label} />
  {#if group.kind === "folder"}
    {#if folded}<FolderIcon class="size-4 shrink-0 text-fg-muted" />{:else}<FolderOpenIcon
        class="size-4 shrink-0 text-fg-muted" />{/if}
  {/if}
  <div class="name-count gap-2">
    {#if group.kind === "user"}
      <Username name={group.username} />
    {:else if group.kind === "folder"}
      <span class="truncate font-mono text-xs" title={group.label}>{group.label}</span>
    {:else}
      <span class="truncate font-medium">{group.label}</span>
    {/if}
    <span class="text-xs text-fg-muted"
      ><span
        >{plural(list.length, "file", "files")} · {formatBytes(bytes)}{group.kind === "batch"
          ? ` · ${done} done, ${failed} failed`
          : ""}</span
      ></span>
  </div>
  <!-- a phone leaves the progress to the file rows, which show it too -->
  {#if !viewport.phone && (progress.running || (progress.moved > 0 && progress.moved < progress.size))}
    <span
      class="flex shrink-0 items-center gap-1.5 text-xs text-fg-muted tabular-nums"
      title={`${formatBytes(progress.moved)} of ${formatBytes(progress.size)}`}>
      <span class="h-1.5 w-16 overflow-hidden rounded-full bg-surface-muted ring-1 ring-border">
        <span class="block h-full bg-accent" style:width={`${progress.percent}%`}></span>
      </span>
      {Math.floor(progress.percent)}%{progress.eta !== null
        ? ` · ${formatDuration(progress.eta)} left`
        : ""}
    </span>
  {/if}
  {#if usage}
    <span class={["shrink-0 text-xs", usage.exhausted ? "text-danger" : "text-fg-muted"]}
      >{usage.text}</span>
  {/if}
  <div class="ml-auto flex items-center row-action">
    {#if viewport.phone}
      <!-- a phone keeps the row for the name, so the group's actions sit in a menu -->
      {#if actions.length > 0}
        <Menu.LazyRoot>
          {#snippet trigger({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={`More for ${group.label}`}>
              <EllipsisIcon />
            </Button>
          {/snippet}
          <Menu.Content align="end" class="min-w-56">
            {#each actions as a (a.label)}
              <Menu.Item onSelect={a.run}><a.icon />{a.label}</Menu.Item>
            {/each}
          </Menu.Content>
        </Menu.LazyRoot>
      {/if}
    {:else}
      {#each actions as a (a.label)}
        {#if a.text}
          <Button variant="ghost" size="sm" onclick={a.run}><a.icon />{a.label}</Button>
        {:else}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={a.label}
            title={a.label}
            onclick={a.run}>
            <a.icon />
          </Button>
        {/if}
      {/each}
    {/if}
  </div>
</div>

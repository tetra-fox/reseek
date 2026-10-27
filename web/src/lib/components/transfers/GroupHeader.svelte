<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import FileArchiveIcon from "@lucide/svelte/icons/file-archive";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import FolderSearchIcon from "@lucide/svelte/icons/folder-search";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import UserXIcon from "@lucide/svelte/icons/user-x";
  import XIcon from "@lucide/svelte/icons/x";

  import { root } from "#lib/api/client.js";
  import { remoteFolder } from "#lib/components/files/links.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatBytes, plural } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { act, folderOf, openFolder } from "#lib/transfers/actions.js";
  import { encodePath } from "#lib/transfers/content.js";
  import {
    isCancellable,
    isRetryable,
    isSucceeded,
    type Direction
  } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Group } from "#lib/transfers/view.js";
  import { quota, waits } from "#lib/transfers/why.svelte.js";

  type Props = {
    group: Group;
    store: Transfers;
    direction: Direction;
    folded: boolean;
    ticked: number;
    ontoggle: () => void;
    onselect: (on: boolean) => void;
    /** removes the group's transfers, asking first for many or running ones */
    onremove: () => void;
    /** blocks the user, asking first; with `deny` their unfinished uploads are cancelled first */
    onblock: (deny: boolean) => void;
  };

  let { group, store, direction, folded, ticked, ontoggle, onselect, onremove, onblock }: Props =
    $props();

  const write = $derived(session.can("ReadWrite"));
  const list = $derived(group.transfers);
  const bytes = $derived(list.reduce((n, t) => n + t.size, 0));
  const done = $derived(list.filter((t) => t.state.includes("Completed")).length);
  const failed = $derived(list.filter((t) => isRetryable(t.state)).length);
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
    if (local === null) return;
    if (list.some((t) => !isSucceeded(t.state))) toast("Only completed files are included");
    const name = local.split("/").pop() || "downloads";
    window.location.assign(
      `${root()}/api/v0/files/downloads/directories/${encodePath(local)}/archive?name=${encodeURIComponent(name)}`
    );
  };
</script>

<div
  class="flex h-full items-center gap-2 border-b bg-surface-muted/80 px-2 text-sm"
  style:padding-left={`${0.5 + group.depth * 1.5}rem`}
  role="rowheader">
  <Checkbox
    aria-label={`Select every transfer in ${group.label}`}
    checked={ticked > 0 && ticked === list.length}
    indeterminate={ticked > 0 && ticked < list.length}
    onCheckedChange={onselect} />
  <button
    type="button"
    aria-label={folded ? "Unfold" : "Fold"}
    aria-expanded={!folded}
    onclick={ontoggle}>
    {#if folded}<ChevronRightIcon class="size-4" />{:else}<ChevronDownIcon class="size-4" />{/if}
  </button>
  {#if group.kind === "user"}
    <Username name={group.username} />
  {:else if group.kind === "folder"}
    {#if folded}<FolderIcon class="size-4 text-fg-muted" />{:else}<FolderOpenIcon
        class="size-4 text-fg-muted" />{/if}
    <span class="min-w-0 truncate font-mono text-xs" title={group.label}>{group.label}</span>
  {:else}
    <span class="font-medium">{group.label}</span>
  {/if}
  <span class="shrink-0 text-xs text-fg-muted">
    {plural(list.length, "file", "files")} · {formatBytes(bytes)}{group.kind === "batch"
      ? ` · ${done} done, ${failed} failed`
      : ""}
  </span>
  {#if usage}
    <span class={["shrink-0 text-xs", usage.exhausted ? "text-danger" : "text-fg-muted"]}
      >{usage.text}</span>
  {/if}
  <div class="ml-auto flex items-center row-action">
    {#if write && direction === "download" && list.some((t) => isRetryable(t.state))}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Retry the group"
        onclick={() =>
          void act(
            store,
            "retry",
            list.filter((t) => isRetryable(t.state)).map((t) => t.id)
          )}>
        <RotateCwIcon />
      </Button>
    {/if}
    {#if write && list.some((t) => isCancellable(t.state))}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Cancel the group"
        onclick={() =>
          void act(
            store,
            "cancel",
            list.filter((t) => isCancellable(t.state)).map((t) => t.id)
          )}>
        <XIcon />
      </Button>
    {/if}
    {#if write}
      <Button variant="ghost" size="icon-sm" aria-label="Remove the group" onclick={onremove}>
        <Trash2Icon />
      </Button>
    {/if}
    {#if direction === "download" && group.kind === "folder"}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Download more from this folder"
        title="Download more from this folder"
        onclick={() => goto(remoteFolder(group.username, list[0].filename).browse)}>
        <FolderSearchIcon />
      </Button>
      {#if local !== null}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Open folder"
          title="Open folder"
          onclick={() => openFolder(list)}>
          <FolderOpenIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Download folder"
          title="Download folder"
          onclick={zip}>
          <FileArchiveIcon />
        </Button>
      {/if}
    {/if}
    {#if write && direction === "upload" && group.kind === "user"}
      <Button
        variant="ghost"
        size="sm"
        onclick={() =>
          void act(
            store,
            "cancel",
            list.filter((t) => t.state.includes("Queued")).map((t) => t.id)
          )}>
        <UserXIcon />Deny all
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={`Block ${group.username}`}
        onclick={() => onblock(false)}>
        <BanIcon />
      </Button>
    {/if}
  </div>
</div>

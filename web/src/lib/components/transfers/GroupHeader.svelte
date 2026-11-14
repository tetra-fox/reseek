<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";

  import TreeToggle from "#lib/components/files/TreeToggle.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatBytes, formatCount, formatDuration, namedOf, plural } from "#lib/format.js";
  import { groupProgress } from "#lib/transfers/grid/progress.js";
  import type { Group } from "#lib/transfers/grid/view.js";
  import { type GroupHandlers, groupActions } from "#lib/transfers/groupActions.js";
  import { type Direction, isEnded, isQueued, isRetryable } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import { quota, waits } from "#lib/transfers/why.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { GAP_REM, lineStart } from "#lib/tree/indent.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import GroupItems from "./GroupItems.svelte";

  type Props = GroupHandlers & {
    group: Group;
    store: Transfers;
    direction: Direction;
    /** which groups are open */
    expansion: Expansion;
    ticked: number;
    onselect: (on: boolean) => void;
  };

  let { group, store, direction, expansion, ticked, onselect, onremove, oncancel, onblock }: Props =
    $props();

  const list = $derived(group.transfers);
  const bytes = $derived(list.reduce((n, t) => n + t.size, 0));
  const done = $derived(list.filter((t) => isEnded(t.state)).length);
  const failed = $derived(list.filter((t) => isRetryable(t.state)).length);
  const progress = $derived(groupProgress(list));
  const folded = $derived(!expansion.isOpen(group.key));
  // a user's quota, from why the first of their queued uploads waits
  const waiting = $derived(
    direction === "upload" && group.kind === "user"
      ? list.find((t) => isQueued(t.state))
      : undefined
  );
  $effect(() => {
    if (waiting) void waits.load(waiting);
  });
  const usage = $derived(waiting && waits.get(waiting.id) ? quota(waits.get(waiting.id)!) : null);

  const actions = $derived(groupActions(group, store, direction, { onremove, oncancel, onblock }));
</script>

<div
  class="flex h-full items-center px-2 text-sm"
  style:padding-left={`${lineStart(group.depth)}rem`}
  style:column-gap={`${GAP_REM}rem`}
  role="rowheader">
  <Checkbox
    aria-label={`Select every transfer in ${group.label}`}
    checked={ticked > 0 && ticked === list.length}
    indeterminate={ticked > 0 && ticked < list.length}
    onCheckedChange={onselect} />
  <TreeToggle
    {expansion}
    key={group.key}
    siblings={group.siblings}
    inside={group.inside}
    name={group.label} />
  {#if group.kind === "folder"}
    {#if folded}<FolderIcon class="size-4 shrink-0 text-fg-muted" />{:else}<FolderOpenIcon
        class="size-4 shrink-0 text-fg-muted" />{/if}
  {/if}
  <div class="name-count gap-2">
    {#if group.kind === "user"}
      <Username name={group.username} />
    {:else if group.kind === "folder"}
      <Tooltip text={group.folder ?? group.label}>
        {#snippet child({ props })}
          <span {...props} class="flex min-w-0 font-mono text-xs">
            <MiddleTruncate text={group.label} path />
          </span>
        {/snippet}
      </Tooltip>
    {:else if group.job}
      {@const { names, others } = namedOf(group.job.from)}
      <span class="truncate font-medium"
        >{`${group.job.name} from `}{#each names as name, i (name)}{#if i}{" and "}{/if}<Username
            {name} />{/each}{others}</span>
    {:else}
      <span class="truncate font-medium">{group.label}</span>
    {/if}
    <span class="text-xs text-fg-muted"
      ><span
        >{plural(list.length, "file", "files")} · {formatBytes(bytes)}{group.kind === "job"
          ? ` · ${formatCount(done)} done, ${formatCount(failed)} failed`
          : ""}</span
      ></span>
  </div>
  <!-- a phone leaves the progress to the file rows, which show it too -->
  {#if !viewport.phone && (progress.running || (progress.moved > 0 && progress.moved < progress.size))}
    <Tooltip text={`${formatBytes(progress.moved)} of ${formatBytes(progress.size)}`} dense>
      {#snippet child({ props })}
        <span
          {...props}
          class="flex shrink-0 items-center gap-1.5 font-mono text-xs text-fg-muted tabular-nums">
          <Progress
            value={progress.percent}
            label={`Progress of ${group.label}`}
            class="w-16 ring-1 ring-border" />
          {Math.floor(progress.percent)}%{progress.eta !== null
            ? ` · ${formatDuration(progress.eta)} left`
            : ""}
        </span>
      {/snippet}
    </Tooltip>
  {/if}
  <!-- the quota takes only the room the name leaves, from no width of its own -->
  {#if usage}
    <Tooltip text={usage.text} clipped>
      {#snippet child({ props })}
        <span
          {...props}
          class={[
            "min-w-0 flex-1 basis-0 truncate text-xs",
            usage.exhausted ? "text-danger" : "text-fg-muted"
          ]}>{usage.text}</span>
      {/snippet}
    </Tooltip>
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
            <GroupItems {actions} />
          </Menu.Content>
        </Menu.LazyRoot>
      {/if}
    {:else}
      {#each actions as a (a.label)}
        {#if a.text}
          <Button variant="ghost" size="sm" onclick={a.run}><a.icon />{a.label}</Button>
        {:else}
          <Tooltip text={a.label}>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label={a.label}
                onclick={a.run}>
                <a.icon />
              </Button>
            {/snippet}
          </Tooltip>
        {/if}
      {/each}
    {/if}
  </div>
</div>

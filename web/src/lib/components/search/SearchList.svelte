<script lang="ts">
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import LockIcon from "@lucide/svelte/icons/lock";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import SearchIcon from "@lucide/svelte/icons/search";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatCount, NONE, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { href } from "#lib/nav.js";
  import { isDone, openSearch, scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";

  import StatusIcon from "./StatusIcon.svelte";

  /** how many rows show at first and how many more each press adds */
  const PAGE = 100;

  keepCurrent(
    () => void searches.load(),
    () => "search",
    searches.list.freeze
  );
  // a stream reopened for this topic resumes where it was and sends no list, so the page asks
  void searches.load();

  let shown = $state(PAGE);
  const working = new SvelteSet<string>();

  const canWrite = $derived(session.can("ReadWrite"));
  // newest first; a search's start never changes, so a row never moves once it is placed
  const rows = $derived(
    searches.list.rows
      .filter((s) => !searches.hidden.has(s.id))
      .toSorted((a, b) => Date.parse(b.startedAt) - Date.parse(a.startedAt))
  );
  const completed = $derived(rows.filter((s) => isDone(s.state)));

  const stop = async (id: string) => {
    if (working.has(id)) return;
    working.add(id);
    const outcome = await searches.stop(id);
    working.delete(id);
    if (!outcome.ok) toast(outcome.error, "error");
  };
</script>

{#if !searches.loaded && searches.error}
  <ErrorSegment caption={searches.error} retry={() => void searches.load()} retryLabel="Retry" />
{:else if !searches.loaded}
  <div class="flex flex-col gap-2 rounded-card border bg-surface p-4" aria-hidden="true">
    {#each [0, 1, 2, 3] as n (n)}<Skeleton class="h-6" />{/each}
  </div>
{:else if rows.length === 0}
  <EmptyState icon={SearchIcon} title="No searches yet" />
{:else}
  <Card title="Searches">
    {#snippet action()}
      {#if canWrite}
        <Button
          variant="outline"
          size="sm"
          disabled={!completed.length}
          onclick={() => searches.removeLater(completed.map((c) => c.id))}>
          <Trash2Icon />Delete completed searches
        </Button>
      {/if}
    {/snippet}
    {#snippet flush()}
      <div class="overflow-x-auto">
        <table class="data-table w-full text-sm">
          <thead>
            <tr>
              <th class="w-10"><span class="sr-only">State</span></th>
              <th>Search</th>
              <th class="text-right max-md:hidden">Files</th>
              <th class="text-right max-md:hidden">Locked</th>
              <th class="text-right max-md:hidden">Users</th>
              <th class="max-md:hidden">Ended</th>
              <th class="w-20"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody {@attach freezesWhileUsed(searches.list.freeze)}>
            {#each rows.slice(0, shown) as search (search.id)}
              {@const done = isDone(search.state)}
              {@const scope = scopeLabel(search.scope)}
              <tr
                class={["group", working.has(search.id) && "cursor-wait opacity-60"]}
                data-id={search.id}>
                <td><StatusIcon state={search.state} /></td>
                <td class="max-w-0 min-w-32 whitespace-normal md:min-w-48">
                  <a
                    href={href(`/searches/${search.id}`)}
                    dir="auto"
                    class="font-medium wrap-anywhere hover:underline">{search.searchText}</a>
                  {#if scope}
                    <Tooltip text={scope} clipped>
                      {#snippet child({ props })}
                        <Badge {...props} tone="accent" class="ml-1">{scope}</Badge>
                      {/snippet}
                    </Tooltip>
                  {/if}
                  <!-- a phone has no room for the figures' columns -->
                  <span class="block text-xs text-fg-muted md:hidden">
                    {plural(search.fileCount, "file", "files")}{#if search.endedAt}{" · "}<Time
                        at={search.endedAt} />{/if}
                  </span>
                </td>
                <td class="text-right tabular-nums max-md:hidden">
                  {formatCount(search.fileCount)}
                </td>
                <td class="text-right text-fg-muted tabular-nums max-md:hidden">
                  <LockIcon class="inline size-3 text-warning-icon" />
                  {formatCount(search.lockedFileCount)}
                </td>
                <td class="text-right tabular-nums max-md:hidden">
                  {formatCount(search.responseCount)}
                </td>
                <td class="text-fg-muted tabular-nums max-md:hidden">
                  {#if search.endedAt}<Time at={search.endedAt} />{:else}{NONE}{/if}
                </td>
                <td class="text-right">
                  {#if working.has(search.id)}
                    <Spinner class="size-4" />
                  {:else if canWrite}
                    <span class="inline-flex row-action">
                      <Tooltip text="Search again">
                        {#snippet child({ props })}
                          <Button
                            {...props}
                            variant="ghost"
                            size="icon-sm"
                            aria-label="Search again"
                            disabled={!app.isServerConnected}
                            onclick={() => openSearch(search.searchText, search.scope)}>
                            <RefreshCwIcon />
                          </Button>
                        {/snippet}
                      </Tooltip>
                      <Tooltip text={done ? "Delete" : "Stop"}>
                        {#snippet child({ props })}
                          <Button
                            {...props}
                            variant="ghost"
                            size="icon-sm"
                            class="text-danger"
                            aria-label={done ? "Delete" : "Stop"}
                            onclick={() =>
                              done ? searches.removeLater([search.id]) : void stop(search.id)}>
                            {#if done}<Trash2Icon />{:else}<CircleStopIcon />{/if}
                          </Button>
                        {/snippet}
                      </Tooltip>
                    </span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if rows.length > shown}
        <div class="border-t p-3 text-center">
          <Button variant="outline" onclick={() => (shown += PAGE)}>
            Show {formatCount(Math.min(PAGE, rows.length - shown))} more ({formatCount(
              rows.length - shown
            )} not shown)
          </Button>
          <Button variant="ghost" onclick={() => (shown = rows.length)}>Show all</Button>
        </div>
      {/if}
    {/snippet}
  </Card>
{/if}

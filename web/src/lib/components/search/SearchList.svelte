<script lang="ts">
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import LockIcon from "@lucide/svelte/icons/lock";
  import SearchIcon from "@lucide/svelte/icons/search";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { SvelteSet } from "svelte/reactivity";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { dateTime } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { href } from "#lib/nav.js";
  import { isDone, scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import StatusIcon from "./StatusIcon.svelte";

  /** how many rows show at first and how many more each press adds */
  const PAGE = 100;

  keepCurrent(
    () => void searches.load(),
    () => "search"
  );
  // a stream reopened for this topic resumes where it was and sends no list, so the page asks
  void searches.load();

  let shown = $state(PAGE);
  let confirming = $state(false);
  let clearing = $state(false);
  const working = new SvelteSet<string>();

  const canWrite = $derived(session.can("ReadWrite"));
  // newest first; a search's start never changes, so a row never moves once it is placed
  const rows = $derived(
    searches.list.rows.toSorted((a, b) => Date.parse(b.startedAt) - Date.parse(a.startedAt))
  );
  const completed = $derived(rows.filter((s) => isDone(s.state)));

  const act = async (id: string, done: boolean) => {
    if (working.has(id)) return;
    working.add(id);
    const outcome = done ? await searches.remove(id) : await searches.stop(id);
    working.delete(id);
    if (!outcome.ok) toast(outcome.error, "error");
  };

  /** Deletes every completed search, one after another; there is no call for several at once */
  const clearCompleted = async () => {
    clearing = true;
    for (const search of completed) {
      const outcome = await searches.remove(search.id);
      if (!outcome.ok) {
        toast(outcome.error, "error");
        break;
      }
    }
    clearing = false;
  };
</script>

{#if !searches.loaded && searches.error}
  <ErrorSegment caption={searches.error} retry={() => void searches.load()} retryLabel="Retry" />
{:else if !searches.loaded}
  <div class="flex justify-center py-10"><Spinner class="size-8" /></div>
{:else if rows.length === 0}
  <EmptyState icon={SearchIcon} title="No searches to display" />
{:else}
  <Card title="Searches" description={`${rows.length} kept`}>
    {#snippet action()}
      {#if canWrite}
        <Button
          variant="outline"
          size="sm"
          disabled={!completed.length || clearing}
          onclick={() => (confirming = true)}>
          <Trash2Icon />Delete all completed
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
              <th class="text-right">Files</th>
              <th class="text-right">Locked</th>
              <th class="text-right">Responses</th>
              <th>Ended</th>
              <th class="w-12"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody {@attach freezesWhileUsed}>
            {#each rows.slice(0, shown) as search (search.id)}
              {@const done = isDone(search.state)}
              {@const scope = scopeLabel(search.scope)}
              <tr
                class={["group", working.has(search.id) && "cursor-wait opacity-60"]}
                data-id={search.id}>
                <td><StatusIcon state={search.state} /></td>
                <td class="max-w-0 min-w-48 whitespace-normal">
                  <a
                    href={href(`/searches/${search.id}`)}
                    class="font-medium wrap-anywhere hover:underline">{search.searchText}</a>
                  {#if scope}<Badge tone="accent" class="ml-1">{scope}</Badge>{/if}
                </td>
                <td class="text-right tabular-nums">{search.fileCount.toLocaleString()}</td>
                <td class="text-right text-fg-muted tabular-nums">
                  <LockIcon class="inline size-3 text-warning" />
                  {search.lockedFileCount.toLocaleString()}
                </td>
                <td class="text-right tabular-nums">{search.responseCount.toLocaleString()}</td>
                <td class="text-fg-muted tabular-nums">
                  {search.endedAt ? dateTime(search.endedAt) : "-"}
                </td>
                <td class="text-right">
                  {#if working.has(search.id)}
                    <Spinner class="size-4" />
                  {:else if canWrite}
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      class="text-danger row-action"
                      aria-label={done ? "Delete" : "Stop"}
                      onclick={() => void act(search.id, done)}>
                      {#if done}<Trash2Icon />{:else}<CircleStopIcon />{/if}
                    </Button>
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
            Show {Math.min(PAGE, rows.length - shown)} more ({rows.length - shown} not shown)
          </Button>
        </div>
      {/if}
    {/snippet}
  </Card>
{/if}

<ConfirmDialog
  bind:open={confirming}
  title="Delete all completed searches?"
  body={`The ${completed.length} completed searches and their results are deleted. Running searches stay.`}
  confirm="Delete"
  danger
  onconfirm={() => void clearCompleted()} />

<script lang="ts">
  import HistoryIcon from "@lucide/svelte/icons/history";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { formatDate, getFileName, plural } from "#lib/format.js";
  import { call } from "#lib/request.js";
  import { isSucceeded } from "#lib/transfers/states.js";

  // a newest-first timeline of what happened with one user: transfers, messages by day, blocks
  let { username }: { username: string } = $props();

  const PAGE = 50;
  /** how an ended transfer's state reads in a sentence, where its own name doesn't */
  const ENDINGS: Record<string, string> = { Errored: "failed", TimedOut: "timed out" };
  type Entry = { key: string; at: string; text: string };

  let wanted = $state(PAGE);
  let entries = $state.raw<Entry[]>([]);
  let more = $state(false);
  let loaded = $state(false);
  let errors = $state<string[]>([]);

  const transferEntry = (t: {
    id: string;
    direction: string;
    filename: string;
    state: string;
    requestedAt: string;
    endedAt?: string | null;
    exception?: string | null;
  }): Entry => {
    const name = getFileName(t.filename);
    const what = t.direction === "Upload" ? "Upload of" : "Download of";
    if (!t.endedAt)
      return { key: `t:${t.id}`, at: t.requestedAt, text: `${what} ${name} requested` };
    if (isSucceeded(t.state)) {
      return {
        key: `t:${t.id}`,
        at: t.endedAt,
        text: `${t.direction === "Upload" ? "Uploaded" : "Downloaded"} ${name}`
      };
    }
    const ending = t.state.replace("Completed, ", "");
    const outcome = ENDINGS[ending] ?? ending.toLowerCase();
    return {
      key: `t:${t.id}`,
      at: t.endedAt,
      text: `${what} ${name} ${outcome}${t.exception ? `: ${t.exception}` : ""}`
    };
  };

  const load = async () => {
    const limit = wanted;
    const [transfers, messages, events] = await Promise.all([
      call(() =>
        api.GET("/api/v0/users/{username}/transfers", {
          params: { path: { username }, query: { ...byUser(username).query, limit } }
        })
      ),
      call(() =>
        api.GET("/api/v0/conversations/{username}/messages", {
          params: { path: { username }, query: { ...byUser(username).query, limit } }
        })
      ),
      call(() =>
        api.GET("/api/v0/events", {
          params: { query: { username, type: "UserBlocked,UserUnblocked", limit } }
        })
      )
    ]);
    const found: Entry[] = [];
    if (transfers.ok) found.push(...transfers.data.transfers.map(transferEntry));
    // a missing conversation is no messages, not a failure
    if (messages.ok) {
      const days = new Map<string, { at: string; n: number }>();
      for (const m of messages.data) {
        const day = new Date(m.timestamp).toDateString();
        const seen = days.get(day);
        if (!seen) days.set(day, { at: m.timestamp, n: 1 });
        else {
          seen.n += 1;
          if (m.timestamp > seen.at) seen.at = m.timestamp;
        }
      }
      for (const [day, { at, n }] of days) {
        found.push({ key: `m:${day}`, at, text: plural(n, "message", "messages") });
      }
    }
    if (events.ok) {
      for (const e of events.data) {
        const text =
          e.type === "UserBlocked"
            ? `Blocked by ${e.data.by}${e.data.reason ? `: ${e.data.reason}` : ""}`
            : e.type === "UserUnblocked"
              ? e.data.reason === "expired"
                ? "Block expired"
                : `Unblocked by ${e.data.by}`
              : e.type;
        found.push({ key: `e:${e.id}`, at: e.timestamp, text });
      }
    }
    errors = [transfers, events].flatMap((o) => (o.ok ? [] : [o.error]));
    found.sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
    more =
      (transfers.ok && transfers.data.total > limit) ||
      (messages.ok && messages.data.length >= limit) ||
      (events.ok && events.data.length >= limit);
    entries = found;
    loaded = true;
  };

  $effect(() => {
    void wanted;
    untrack(() => void load());
  });
</script>

{#each errors as error (error)}<Notice kind="error">{error}</Notice>{/each}

{#if !loaded}
  <Skeleton class="h-40" />
{:else if !entries.length}
  <EmptyState icon={HistoryIcon} title="No activity with this user yet" />
{:else}
  <ol class="flex flex-col rounded-card border bg-surface shadow-card">
    {#each entries as entry (entry.key)}
      <li class="flex gap-4 border-b px-4 py-2 text-sm last:border-b-0">
        <time class="w-40 shrink-0 text-xs text-fg-muted" datetime={entry.at}
          >{formatDate(entry.at)}</time>
        <span class="min-w-0 wrap-anywhere">{entry.text}</span>
      </li>
    {/each}
  </ol>
  {#if more}
    <Button variant="outline" class="self-center" onclick={() => (wanted += PAGE)}
      >Load more</Button>
  {/if}
{/if}

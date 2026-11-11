<script lang="ts">
  import HistoryIcon from "@lucide/svelte/icons/history";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { formatDate, getFileName, plural } from "#lib/format.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { endingText, isSucceeded, type TransferState } from "#lib/transfers/states.js";

  // a newest-first timeline of what happened with one user: transfers, messages by day, blocks
  let { username }: { username: string } = $props();

  const PAGE = 50;
  type Entry = { key: string; at: string; text: string };

  let wanted = $state(PAGE);

  const transferEntry = (t: {
    id: string;
    direction: string;
    filename: string;
    state: TransferState;
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
    const outcome = endingText(t.state);
    return {
      key: `t:${t.id}`,
      at: t.endedAt,
      text: `${what} ${name} ${outcome}${t.exception ? `: ${t.exception}` : ""}`
    };
  };

  /** the newest `wanted` of each kind, merged; what could not be read is said beside them */
  const activity = new Loader(async () => {
    const limit = wanted;
    const [transfers, messages, events] = await Promise.all([
      call(() =>
        api.GET("/api/v0/users/{username}/transfers", {
          params: { path: byUser(username).path, query: { ...byUser(username).query, limit } }
        })
      ),
      call(() =>
        api.GET("/api/v0/conversations/{username}/messages", {
          params: { path: byUser(username).path, query: { ...byUser(username).query, limit } }
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
    found.sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
    const more =
      (transfers.ok && transfers.data.total > limit) ||
      (messages.ok && messages.data.length >= limit) ||
      (events.ok && events.data.length >= limit);
    const errors = [transfers, events].flatMap((o) => (o.ok ? [] : [o.error]));
    return { ok: true as const, data: { entries: found, more, errors } };
  });
  const entries = $derived(activity.data?.entries ?? []);

  $effect(() => {
    void wanted;
    untrack(() => void activity.load());
  });
</script>

{#each activity.data?.errors ?? [] as error (error)}<Notice kind="error">{error}</Notice>{/each}

{#if activity.loading}
  <Skeleton class="h-40" />
{:else if !entries.length}
  <EmptyState icon={HistoryIcon} title="No activity with this user yet" />
{:else}
  <ol class="flex flex-col rounded-card border bg-surface">
    {#each entries as entry (entry.key)}
      <li class="flex gap-4 border-b px-4 py-2 text-sm last:border-b-0">
        <time class="w-40 shrink-0 text-xs text-fg-muted" datetime={entry.at}
          >{formatDate(entry.at)}</time>
        <span class="min-w-0 wrap-anywhere">{entry.text}</span>
      </li>
    {/each}
  </ol>
  {#if activity.data?.more}
    <Button variant="outline" class="self-center" onclick={() => (wanted += PAGE)}
      >Load more</Button>
  {/if}
{/if}

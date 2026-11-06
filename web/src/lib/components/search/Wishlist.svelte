<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { plural } from "#lib/format.js";
  import { jobs } from "#lib/jobs/jobs.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { href } from "#lib/nav.js";
  import { wishlist } from "#lib/search/wishlist.svelte.js";
  import { session } from "#lib/session.svelte.js";

  // saved searches the daemon runs again on the server's interval, telling of new files; read
  // again after each turn, which the wishlist's schedule card says ended, and as the screen
  // keeps current
  keepCurrent(() => void wishlist.load());
  const write = $derived(session.can("ReadWrite"));
  const busy = new Busy();
  let text = $state("");
  const items = $derived(wishlist.rows.rows);
  const schedule = $derived(jobs.schedules.find((s) => s.name === "wishlist"));
  const turned = $derived(schedule?.lastRunAt);
  $effect(() => {
    void turned;
    untrack(() => void wishlist.load());
  });

  const add = async (e: SubmitEvent) => {
    e.preventDefault();
    const typed = text.trim();
    if (!typed) return;
    const outcome = await busy.run(() => wishlist.add(typed));
    if (outcome?.ok) text = "";
  };
</script>

<Card
  title="Wishlist"
  description={schedule?.every
    ? "Each search runs again in turn, as often as the Soulseek server allows, and tells you when it finds new files."
    : app.isServerConnected
      ? "Each search runs again in turn once the Soulseek server says how often it may."
      : "Each search runs again in turn while you are connected."}>
  {#if write}
    <form class="flex gap-inline px-5" onsubmit={add}>
      <Input
        placeholder="Add a search to the wishlist"
        aria-label="Search to add to the wishlist"
        maxlength={256}
        wrapperClass="flex-1"
        bind:value={text} />
      <Button type="submit" variant="outline" disabled={!text.trim() || busy.running}
        ><PlusIcon />Add</Button>
    </form>
  {/if}
  {#if wishlist.error && !items.length}
    <p class="px-5 text-sm text-danger">{wishlist.error}</p>
  {:else if wishlist.loaded && !items.length}
    <p class="px-5 text-sm text-fg-muted">Nothing on the wishlist yet.</p>
  {:else if items.length}
    <ul class="divide-y border-t text-sm">
      {#each items as w (w.id)}
        <li class="flex min-w-0 items-center gap-3 px-5 py-2">
          <span class="min-w-0 flex-1 truncate font-medium" title={w.searchText}
            >{w.searchText}</span>
          {#if w.lastSearchedAt}
            <span class="flex shrink-0 items-center gap-1.5 text-xs text-fg-muted">
              {#if w.lastNewFiles}
                <Badge tone="success">{plural(w.lastNewFiles, "new file", "new files")}</Badge>
              {/if}
              {#if w.lastSearchId}
                <a class="hover:underline" href={href(`/searches/${w.lastSearchId}`)}
                  >Searched <Time at={w.lastSearchedAt} /></a>
              {:else}
                <span>Searched <Time at={w.lastSearchedAt} /></span>
              {/if}
            </span>
          {:else}
            <span class="shrink-0 text-xs text-fg-muted">Not searched yet</span>
          {/if}
          {#if write}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Remove ${w.searchText} from the wishlist`}
              title="Remove from the wishlist"
              disabled={busy.running}
              onclick={() => void busy.run(() => wishlist.remove(w.id))}><Trash2Icon /></Button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Card>

<script lang="ts">
  import { goto } from "$app/navigation";
  import MessageSquareDashedIcon from "@lucide/svelte/icons/message-square-dashed";

  import { app } from "#lib/app.svelte.js";
  import type { RoomMembers, WallLine } from "#lib/chat/room.svelte.js";
  import { clock } from "#lib/clock.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { ago } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  // the room's wall: each member's ticker, the latest change first, and a field to set our own
  let { members }: { members: RoomMembers } = $props();

  const us = $derived(app.state?.user.username ?? "");
  const mine = $derived(members.wall.find((l) => l.username === us)?.ticker ?? "");
  const write = $derived(session.can("ReadWrite"));
  const list = new ListControl<WallLine>({
    lines: () => members.wall,
    key: (l) => l.username,
    id: (l) => l.username,
    onopen: (l) => void goto(userHref(l.username))
  });

  // what is typed, starting over from ours whenever it changes
  let text = $derived(mine);
  let saving = $state(false);

  const save = async (value: string) => {
    saving = true;
    const outcome = await members.setTicker(value.trim());
    saving = false;
    if (!outcome.ok) return void toast(outcome.error, "error");
    toast(value.trim() ? "Ticker set" : "Ticker removed");
  };
</script>

<section class="flex min-h-0 w-full flex-col gap-2 p-2" aria-label="Room wall">
  {#if write}
    <form
      class="flex flex-col gap-1"
      onsubmit={(e) => {
        e.preventDefault();
        void save(text);
      }}>
      <Input
        maxlength={200}
        placeholder="Your ticker in this room"
        aria-label="Your ticker"
        bind:value={text} />
      <div class="flex justify-end gap-1">
        {#if mine}
          <Button size="sm" variant="ghost" disabled={saving} onclick={() => void save("")}
            >Remove</Button>
        {/if}
        <Button size="sm" type="submit" disabled={saving || text.trim() === mine}>Set</Button>
      </div>
    </form>
  {/if}
  <div
    class="min-h-0 flex-1 overflow-y-auto rounded-control outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Tickers"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    {#each members.wall as line, i (line.username)}
      <div
        role="row"
        {...list.line(line, i)}
        class={[
          "list-row px-1 py-1.5 text-sm",
          list.selected.has(line.username) && "bg-accent-soft/60"
        ]}>
        <div role="gridcell">
          <div class="flex items-baseline gap-2">
            <Username name={line.username} card class="text-xs" />
            {#if line.at}<span class="ml-auto text-xs text-fg-subtle"
                >{ago(new Date(line.at).toISOString(), clock.now)}</span
              >{/if}
          </div>
          <p class="cursor-text text-sm wrap-anywhere">{line.ticker}</p>
        </div>
      </div>
    {:else}
      <div>
        <EmptyState icon={MessageSquareDashedIcon} title="No tickers yet" />
      </div>
    {/each}
  </div>
</section>

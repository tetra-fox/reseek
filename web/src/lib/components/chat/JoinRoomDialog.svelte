<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import CheckIcon from "@lucide/svelte/icons/check";
  import CrownIcon from "@lucide/svelte/icons/crown";
  import GavelIcon from "@lucide/svelte/icons/gavel";
  import LockIcon from "@lucide/svelte/icons/lock";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";

  import { api, type Schemas } from "#lib/api/client.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Field from "#lib/components/Field.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { ListControl } from "#lib/list.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { roomHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";

  // the server's rooms to pick one from, or a name to join or create
  let { open = $bindable(), name: start = "" }: { open: boolean; name?: string } = $props();

  /** rooms listed at once; the filter narrows the rest */
  const SHOWN = 300;

  const available = new Loader(() => call(() => api.GET("/api/v0/rooms/available")));
  let filter = $state("");
  let name = $state("");
  let isPrivate = $state(false);
  let sort = $state<{ by: "name" | "userCount"; dir: "asc" | "desc" }>({
    by: "userCount",
    dir: "desc"
  });
  let error = $state("");
  let joining = $state(false);

  // each opening reads the list and starts from the name it was opened with
  $effect(() => {
    if (!open) return;
    name = start;
    filter = "";
    error = "";
    isPrivate = false;
    void available.load();
  });

  const rooms = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    const sign = sort.dir === "asc" ? 1 : -1;
    return (available.data ?? [])
      .filter((r) => !f || r.name.toLowerCase().includes(f))
      .toSorted(
        (a, b) =>
          sign * (sort.by === "name" ? a.name.localeCompare(b.name) : a.userCount - b.userCount) ||
          a.name.localeCompare(b.name)
      );
  });
  const shown = $derived(rooms.slice(0, SHOWN));

  const list = new ListControl<Schemas["RoomInfo"]>({
    lines: () => shown,
    key: (r) => r.name,
    id: (r) => r.name,
    onopen: (r) => void join(r.name)
  });
  // a row picked fills the name
  $effect(() => {
    const picked = [...list.selected];
    if (picked.length === 1) name = picked[0];
  });

  const sortBy = (by: "name" | "userCount") => {
    sort = {
      by,
      dir: sort.by === by ? (sort.dir === "asc" ? "desc" : "asc") : by === "name" ? "asc" : "desc"
    };
  };

  const join = async (room: string) => {
    if (!room.trim() || joining) return;
    joining = true;
    const outcome = await chat.join(room.trim(), isPrivate);
    joining = false;
    if (!outcome.ok) return void (error = outcome.error);
    open = false;
    void goto(roomHref(room.trim()));
  };
</script>

{#snippet header(label: string, by: "name" | "userCount")}
  <button type="button" class="flex items-center gap-1" onclick={() => sortBy(by)}>
    {label}
    {#if sort.by === by}
      {#if sort.dir === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
          class="size-3" />{/if}
    {/if}
  </button>
{/snippet}

<Dialog bind:open title="Join a room" class="w-[min(36rem,calc(100vw-2rem))]">
  <div class="flex gap-2">
    <Input placeholder="Room filter" aria-label="Room filter" bind:value={filter} />
    <Button
      variant="outline"
      size="icon"
      aria-label="Ask the server again"
      onclick={() => void available.load()}><RefreshCwIcon /></Button>
  </div>
  {#if available.loading}
    <div class="flex items-center justify-center gap-2 py-8 text-sm text-fg-muted">
      <Spinner class="size-5" />Loading Room List
    </div>
  {:else if available.error}
    <Notice kind="error">{available.error}</Notice>
  {:else}
    <div
      class="max-h-72 min-h-32 overflow-y-auto rounded-control border text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
      role="grid"
      aria-label="Rooms"
      aria-activedescendant={list.activeId}
      tabindex="0"
      {@attach list.attach}>
      <div
        role="row"
        class="sticky top-0 grid grid-cols-[1fr_6rem] border-b bg-surface-muted px-2 py-1 text-xs font-medium text-fg-muted">
        <span role="columnheader">{@render header("Name", "name")}</span>
        <span role="columnheader" class="justify-self-end"
          >{@render header("Users", "userCount")}</span>
      </div>
      {#each shown as room, i (room.name)}
        <div
          role="row"
          {...list.line(room, i)}
          class={[
            "grid grid-cols-[1fr_6rem] items-center px-2 py-1 hover:bg-hover",
            list.selected.has(room.name) && "bg-accent-soft/60 font-medium",
            list.isFocused(room) && "ring-1 ring-accent ring-inset"
          ]}>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
            {#if room.joined}<CheckIcon class="size-3.5 text-success" aria-label="joined" />{/if}
            {#if room.isPrivate}<LockIcon
                class="size-3.5 text-fg-muted"
                aria-label="private" />{/if}
            {#if room.isOwned}<CrownIcon class="size-3.5 text-warning" aria-label="yours" />{/if}
            {#if room.isModerated}<GavelIcon
                class="size-3.5 text-fg-muted"
                aria-label="you moderate it" />{/if}
            <span class="truncate">{room.name}</span>
          </span>
          <span role="gridcell" class="text-right tabular-nums"
            >{room.userCount.toLocaleString()}</span>
        </div>
      {/each}
    </div>
    {#if rooms.length > shown.length}
      <p class="text-xs text-fg-muted">
        {rooms.length - shown.length} more; type in the filter to narrow them.
      </p>
    {/if}
  {/if}
  <Field label="Join or create by name" for="join-name">
    <Input id="join-name" maxlength={24} bind:value={name} autocomplete="off" />
  </Field>
  <label class="flex items-center gap-2 text-sm">
    <Checkbox checked={isPrivate} onCheckedChange={(on) => (isPrivate = on)} />
    Private: create it as a private room if it doesn't exist
  </label>
  {#if error}<Notice kind="error">{error}</Notice>{/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
    <Button disabled={!name.trim() || joining} onclick={() => void join(name)}>Join</Button>
  {/snippet}
</Dialog>

<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import CheckIcon from "@lucide/svelte/icons/check";
  import CrownIcon from "@lucide/svelte/icons/crown";
  import GavelIcon from "@lucide/svelte/icons/gavel";
  import LockIcon from "@lucide/svelte/icons/lock";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import { SvelteSet } from "svelte/reactivity";

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { ListControl } from "#lib/list.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href, roomHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { fields } from "#lib/stored.svelte.js";

  // the server's rooms to pick one from, or a name to join or create
  let { open = $bindable(), name: start = "" }: { open: boolean; name?: string } = $props();

  /** whether we take invitations to private rooms, the daemon's `chat` option; on unless set off */
  const invitations = $derived(
    fields(fields(app.options).chat).accept_private_room_invitations !== false
  );

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
    // Enter joins the room on the focused row, or opens it once joined
    onopen: (r) => (isJoined(r.name) ? enter(r.name) : void joinRow(r.name))
  });
  /** joined now, by this page's list, which the server's room list can lag behind */
  const isJoined = (room: string) => !!chat.rooms.get(room);
  /** the rooms whose Join or Leave is under way */
  const busy = new SvelteSet<string>();

  const sortBy = (by: "name" | "userCount") => {
    sort = {
      by,
      dir: sort.by === by ? (sort.dir === "asc" ? "desc" : "asc") : by === "name" ? "asc" : "desc"
    };
  };

  const enter = (room: string) => {
    open = false;
    void goto(roomHref(room));
  };

  /** Joins the room on a row; the row says so and the dialog stays for more */
  const joinRow = async (room: string) => {
    busy.add(room);
    const outcome = await chat.join(room, false);
    busy.delete(room);
    if (!outcome.ok) return void (error = `${room}: ${outcome.error}`);
    error = "";
  };
  const leaveRow = async (room: string) => {
    busy.add(room);
    const outcome = await chat.leave(room);
    busy.delete(room);
    if (!outcome.ok) error = `${room}: ${outcome.error}`;
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
    <Input type="search" placeholder="Filter rooms" aria-label="Filter rooms" bind:value={filter} />
    <Button
      variant="outline"
      size="icon"
      aria-label="Refresh the room list"
      onclick={() => void available.load()}><RefreshCwIcon /></Button>
  </div>
  {#if available.loading}
    <div class="flex items-center justify-center gap-2 py-8 text-sm text-fg-muted">
      <Spinner class="size-5" />Loading rooms...
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
        class="sticky top-0 z-10 grid grid-cols-[1fr_5rem_8.5rem] border-b bg-surface-muted px-2 py-1 text-xs font-medium text-fg-muted">
        <span role="columnheader">{@render header("Name", "name")}</span>
        <span role="columnheader" class="justify-self-end"
          >{@render header("Users", "userCount")}</span>
        <span role="columnheader" class="sr-only">Join</span>
      </div>
      {#each shown as room, i (room.name)}
        <div
          role="row"
          {...list.line(room, i)}
          class={[
            "group grid grid-cols-[1fr_5rem_8.5rem] items-center px-2 py-0.5 hover:bg-hover",
            list.selected.has(room.name) && "bg-accent-soft/60",
            list.isFocused(room) && "ring-1 ring-accent ring-inset"
          ]}>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
            {#if room.isPrivate}<LockIcon
                class="size-3.5 text-fg-muted"
                aria-label="private" />{/if}
            {#if room.isOwned}<CrownIcon class="size-3.5 text-warning" aria-label="yours" />{/if}
            {#if room.isModerated}<GavelIcon
                class="size-3.5 text-fg-muted"
                aria-label="you're an operator" />{/if}
            <span class="truncate">{room.name}</span>
          </span>
          <span role="gridcell" class="text-right tabular-nums"
            >{room.userCount.toLocaleString()}</span>
          <span role="gridcell" class="flex items-center justify-end gap-1">
            {#if busy.has(room.name)}
              <Spinner class="size-4" />
            {:else if isJoined(room.name)}
              <button
                type="button"
                class="flex items-center gap-1 rounded-badge px-1.5 text-xs text-success hover:underline"
                title={`Open ${room.name}`}
                onclick={() => enter(room.name)}><CheckIcon class="size-3.5" />Joined</button>
              <Button
                variant="ghost"
                size="sm"
                class="h-6 px-2 text-xs row-action"
                aria-label={`Leave ${room.name}`}
                onclick={() => void leaveRow(room.name)}>Leave</Button>
            {:else}
              <Button
                variant="outline"
                size="sm"
                class="h-6 px-2.5 text-xs"
                aria-label={`Join ${room.name}`}
                onclick={() => void joinRow(room.name)}>Join</Button>
            {/if}
          </span>
        </div>
      {/each}
    </div>
    {#if rooms.length > shown.length}
      <p class="text-xs text-fg-muted">
        {rooms.length - shown.length} more; type in the filter to narrow them.
      </p>
    {/if}
  {/if}
  <Form
    id="join-room"
    layout="stacked"
    class="flex flex-col gap-group"
    onsubmit={(e) => {
      e.preventDefault();
      void join(name);
    }}>
    <Field label="Join or create by name" for="join-name">
      <Input id="join-name" maxlength={24} bind:value={name} autocomplete="off" />
    </Field>
    <label class="flex items-center gap-2 text-sm">
      <Checkbox checked={isPrivate} onCheckedChange={(on) => (isPrivate = on)} />
      Create as a private room if it doesn't exist
    </label>
    {#if error}<Notice kind="error">{error}</Notice>{/if}
    <p class="text-xs text-fg-muted">
      Invitations to private rooms are {invitations ? "accepted" : "refused"}.
      <a
        class="text-accent hover:underline"
        href={href("/system/settings?q=accept_private_room_invitations")}
        onclick={() => (open = false)}>Change</a>
    </p>
  </Form>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
    <Button type="submit" form="join-room" disabled={!name.trim() || joining}>Join</Button>
  {/snippet}
</Dialog>

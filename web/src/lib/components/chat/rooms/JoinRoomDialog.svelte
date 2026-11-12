<script lang="ts" module>
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the room list's sort and column widths, kept in this browser */
  const view = gridView("reseek.rooms.available", ["name", "users"], { id: "users", dir: "desc" });
</script>

<script lang="ts">
  import { goto } from "$app/navigation";
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
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import { formatCount } from "#lib/format.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href, roomHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { CHAT } from "#lib/ui/bindings.js";
  import { isPressed } from "#lib/ui/keys.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  // two parts: the server's public rooms, each joined from its row, and a room created by name
  let { open = $bindable(), name: start = "" }: { open: boolean; name?: string } = $props();

  /** whether we take invitations to private rooms, the daemon's `chat` option; on unless set off */
  const invitations = $derived(app.option("chat.accept_private_room_invitations") !== false);

  const available = new Loader(() => call(() => api.GET("/api/v0/rooms/available")));
  let filter = $state("");
  let name = $state("");
  let isPrivate = $state(false);
  let error = $state("");
  let creating = $state(false);

  // each opening reads the list and starts from the name it was opened with
  $effect(() => {
    if (!open) return;
    name = start;
    filter = "";
    error = "";
    isPrivate = false;
    void available.load();
  });

  type Room = Schemas["RoomInfo"];
  const matching = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    return (available.data ?? []).filter((r) => !f || r.name.toLowerCase().includes(f));
  });
  const grid = new Grid<Room>({
    columns: () => [
      { id: "name", label: "Name", width: "4rem", grow: true, value: (r) => r.name.toLowerCase() },
      {
        id: "users",
        label: "Users",
        width: "5rem",
        end: true,
        descFirst: true,
        value: (r) => r.userCount
      },
      // set by the list's box: narrower on a phone, where "Joined" shows as its check alone
      { id: "join", label: "Join", width: "var(--join-width)", quiet: true, fixed: true }
    ],
    data: () => matching,
    ...keptIn(view),
    ties: "name"
  });
  const rooms = $derived(grid.rows);
  let headerHeight = $state(0);

  const list = new ListControl<Schemas["RoomInfo"]>({
    lines: () => rooms,
    key: (r) => r.name,
    id: (r) => r.name,
    // Enter joins the room on the focused row, or opens it once joined
    onopen: (r) => (isJoined(r.name) ? enter(r.name) : void joinRow(r.name)),
    rendered: (index) => lines.isDrawn(index),
    reveal: (index) => lines.scrollTo(index)
  });
  // the server lists thousands of rooms; the window draws those in view
  const lines: ListWindow<Room> = new ListWindow(
    () => rooms,
    (r) => r.name,
    () => ({
      estimateSize: () => 1.75 * rem.px,
      overscan: 10,
      scrollMargin: headerHeight,
      scrollPaddingStart: headerHeight,
      sizes: rem.px,
      onchange: () => list.scrolled()
    })
  );
  /** joined now, by this page's list, which the server's room list can lag behind */
  const isJoined = (room: string) => !!chat.rooms.get(room);
  /** the rooms whose Join or Leave is under way */
  const busy = new SvelteSet<string>();

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

  /** a public room already called what the create box holds, which is joined rather than made */
  const existing = $derived((available.data ?? []).find((r) => r.name === name.trim()));

  /** Creates the room named in the box, private if asked, and opens it */
  const create = async () => {
    const room = name.trim();
    if (!room || existing || creating) return;
    creating = true;
    const outcome = await chat.join(room, isPrivate);
    creating = false;
    if (!outcome.ok) return void (error = outcome.error);
    open = false;
    void goto(roomHref(room));
  };

  /** Enter in the filter: joins the focused room, or the first one listed */
  const joinFocused = () => {
    const room = list.focused ?? rooms[0];
    if (!room) return;
    if (isJoined(room.name)) enter(room.name);
    else void joinRow(room.name);
  };
</script>

<Dialog bind:open title="Join room" busy={creating} class="w-[min(36rem,calc(100vw-2rem))]">
  <h3 class="text-sm font-medium">Public rooms</h3>
  <div class="flex gap-2">
    <Input
      type="search"
      placeholder="Filter rooms"
      aria-label="Filter rooms"
      bind:value={filter}
      onkeydown={(e) => {
        if (!isPressed(CHAT.join, e)) return;
        e.preventDefault();
        joinFocused();
      }} />
    <Tooltip text="Refresh room list">
      {#snippet child({ props })}
        <Button
          {...props}
          variant="outline"
          size="icon"
          aria-label="Refresh room list"
          onclick={() => void available.load()}><RefreshCwIcon /></Button>
      {/snippet}
    </Tooltip>
  </div>
  {#if available.loading}
    <div class="flex items-center justify-center gap-2 py-8 text-sm text-fg-muted">
      <Spinner class="size-5" />Loading rooms...
    </div>
  {:else if available.error}
    <Notice kind="error">{available.error}</Notice>
  {:else}
    <div
      class="max-h-72 min-h-32 overflow-y-auto rounded-control border text-sm outline-none [--join-width:6rem] focus-visible:ring-2 focus-visible:ring-accent sm:[--join-width:8.5rem]"
      role="grid"
      aria-label="Rooms"
      aria-activedescendant={list.activeId}
      tabindex="0"
      aria-rowcount={rooms.length}
      {@attach lines.attach}
      {@attach list.attach}>
      <div class="sticky top-0 z-10" bind:offsetHeight={headerHeight}>
        <GridHeader {grid} class="h-7 border-b bg-surface-muted text-fg-muted" />
      </div>
      <div class="relative" style:height={`${lines.total}px`}>
        {#each lines.drawn as { line: room, index, start, size } (room.name)}
          <div
            role="row"
            {...list.line(room, index)}
            style={grid.row}
            style:transform={`translateY(${start - headerHeight}px)`}
            style:height={`${size}px`}
            class="group absolute inset-x-0 top-0 grid items-center line-tint">
            <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
              {#if room.isPrivate}<LockIcon
                  class="size-3.5 shrink-0 text-fg-muted"
                  aria-label="Private" />{/if}
              {#if room.isOwned}<CrownIcon
                  class="size-3.5 shrink-0 text-warning"
                  aria-label="Owned" />{/if}
              {#if room.isModerated}<GavelIcon
                  class="size-3.5 shrink-0 text-fg-muted"
                  aria-label="Operator" />{/if}
              <Tooltip text={room.name} clipped>
                {#snippet child({ props })}
                  <span {...props} class="flex min-w-0"><MiddleTruncate text={room.name} /></span>
                {/snippet}
              </Tooltip>
            </span>
            <span role="gridcell" class="truncate px-2 text-right tabular-nums"
              >{formatCount(room.userCount)}</span>
            <span role="gridcell" class="flex items-center justify-end gap-1 px-2">
              {#if busy.has(room.name)}
                <Spinner class="size-4" />
              {:else if isJoined(room.name)}
                <Tooltip text={`Open ${room.name}`}>
                  {#snippet child({ props })}
                    <button
                      {...props}
                      type="button"
                      class="flex items-center gap-1 rounded-badge px-1.5 text-xs text-success hover:underline"
                      onclick={() => enter(room.name)}
                      ><CheckIcon class="size-3.5" /><span class="max-sm:sr-only">Joined</span
                      ></button>
                  {/snippet}
                </Tooltip>
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
    </div>
  {/if}
  {#if error}<Notice kind="error">{error}</Notice>{/if}
  <Form
    id="create-room"
    layout="stacked"
    class="flex flex-col gap-group border-t pt-group"
    onsubmit={(e) => {
      e.preventDefault();
      void create();
    }}>
    <h3 class="text-sm font-medium">New room</h3>
    <Field label="Name" for="create-name">
      <Input id="create-name" maxlength={24} bind:value={name} autocomplete="off" />
    </Field>
    {#if existing}
      <div class="flex flex-wrap items-center gap-inline text-sm" role="status">
        <span class="text-fg-muted">{existing.name} is already a public room</span>
        {#if isJoined(existing.name)}
          <Button size="sm" variant="outline" onclick={() => enter(existing.name)}>Open</Button>
        {:else}
          <Button
            size="sm"
            variant="outline"
            disabled={busy.has(existing.name)}
            onclick={() => void joinRow(existing.name)}>Join</Button>
        {/if}
      </div>
    {/if}
    <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
      <label class="flex items-center gap-2 text-sm">
        <Switch bind:checked={isPrivate} />
        Private
      </label>
      <Button type="submit" class="ml-auto" disabled={!name.trim() || !!existing || creating}
        >Create</Button>
    </div>
    <p class="text-xs text-fg-muted">
      Only added members can join a private room. Invitations to private rooms are

      {invitations ? "accepted" : "refused"}.
      <a
        class="text-accent hover:underline"
        href={href("/system/settings?q=accept_private_room_invitations")}
        onclick={() => (open = false)}>Change</a>
    </p>
  </Form>
  {#snippet footer()}
    <Button variant="ghost" disabled={creating} onclick={() => (open = false)}>Close</Button>
  {/snippet}
</Dialog>

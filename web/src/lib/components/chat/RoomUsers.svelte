<script lang="ts">
  import { goto } from "$app/navigation";
  import SearchIcon from "@lucide/svelte/icons/search";

  import type { RoomMembers, RoomUser } from "#lib/chat/room.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { flag } from "#lib/components/users/hover.svelte.js";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatSpeed, plural } from "#lib/format.js";
  import { freezesWhileUsed, heldOrder } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { users } from "#lib/users.svelte.js";

  // who is in a room: operators then members, online first, a filter and the counts
  let { members }: { members: RoomMembers } = $props();

  type Line =
    | { kind: "header"; key: string; title: string; names: string[] }
    | { kind: "user"; key: string; user: RoomUser; names: string[] };

  let typed = $state("");

  const PRESENCE = { Online: 0, Away: 1, Offline: 2 } as const;
  const blocked = (name: string) => !!users.get(name)?.block;
  const order = (a: RoomUser, b: RoomUser) =>
    Number(blocked(a.username)) - Number(blocked(b.username)) ||
    PRESENCE[a.status] - PRESENCE[b.status] ||
    a.username.localeCompare(b.username);

  const all = $derived(members.users.rows);
  const online = $derived(all.filter((u) => u.status === "Online").length);
  // the list holds still while it is pressed, selected in or a name's card or menu is open from it
  const hold = heldOrder<Line>((l) => l.key);
  const lines = $derived.by(() => {
    const filter = typed.trim().toLowerCase();
    const shown = all.filter((u) => !filter || u.username.toLowerCase().includes(filter));
    const ops = members.operators;
    const sections = [
      { key: "h:operators", title: "Operators", users: shown.filter((u) => ops.has(u.username)) },
      { key: "h:members", title: "Members", users: shown.filter((u) => !ops.has(u.username)) }
    ];
    return hold(
      sections.flatMap((s): Line[] => {
        if (!s.users.length) return [];
        const sorted = s.users.toSorted(order);
        const names = sorted.map((u) => u.username);
        return [
          { kind: "header", key: s.key, title: s.title, names },
          ...sorted.map((user): Line => ({ kind: "user", key: user.username, user, names }))
        ];
      })
    );
  });

  // the members' blocks come from the directory
  $effect(() => {
    for (const u of all.slice(0, 300)) void users.load(u.username);
  });

  const list = new ListControl<Line>({
    lines: () => lines,
    key: (l) => l.key,
    id: (l) => (l.kind === "user" ? l.user.username : null),
    // a section's own users: Ctrl+A on one selects its section, Space on a header toggles it
    group: (l) => l.names,
    onopen: (l) => {
      if (l.kind === "user") void goto(userHref(l.user.username));
    }
  });
</script>

<aside class="flex min-h-0 flex-col gap-2 border-l p-2" aria-label="Room users">
  <p class="px-1 text-xs text-fg-muted">
    {plural(all.length, "user", "users")} · {online} online
  </p>
  {#if members.error}
    <Notice kind="error">
      {members.error}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => void members.load()}>Retry</Button>
      {/snippet}
    </Notice>
  {/if}
  <div class="relative">
    <SearchIcon class="pointer-events-none absolute top-2 left-2 size-3.5 text-fg-muted" />
    <Input
      class="h-control-sm pl-7"
      placeholder="Filter"
      aria-label="Filter users"
      bind:value={typed} />
  </div>
  <div
    class="min-h-0 flex-1 overflow-y-auto rounded-control outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Users in the room"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    {#each lines as line, i (line.key)}
      {#if line.kind === "header"}
        <div
          role="row"
          {...list.line(line, i)}
          class={[
            "px-1 pt-2 pb-0.5 text-xs font-medium text-fg-muted",
            list.isFocused(line) && "ring-1 ring-accent ring-inset"
          ]}>
          <span role="columnheader">{line.title}</span>
        </div>
      {:else}
        {@const u = line.user}
        {@const country = flag(u.countryCode)}
        <div
          role="row"
          {...list.line(line, i)}
          class={[
            "flex items-center gap-1.5 rounded-badge px-1 py-0.5 text-sm hover:bg-hover",
            list.selected.has(u.username) && "bg-accent-soft/60",
            list.isFocused(line) && "ring-1 ring-accent ring-inset",
            members.users.removed.has(u.username) && "opacity-40"
          ]}>
          <span role="gridcell" class="flex min-w-0 flex-1 items-center gap-1.5">
            <Presence presence={u.status} />
            <span class="w-5 text-center" title={u.countryCode ?? "?"}>{country ?? "?"}</span>
            <span class={["min-w-0 truncate", blocked(u.username) && "line-through"]}>
              <Username name={u.username} />
            </span>
            {#if blocked(u.username)}<span class="size-2 rounded-full bg-danger" title="blocked"
              ></span
              >{/if}
            {#if u.self}<span class="text-xs text-accent">you</span>{/if}
          </span>
          <span role="gridcell" class="shrink-0 text-right text-xs text-fg-muted tabular-nums"
            >{formatSpeed(u.averageSpeed)} · {u.fileCount.toLocaleString()}</span>
        </div>
      {/if}
    {/each}
  </div>
</aside>

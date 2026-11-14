<script lang="ts">
  import { goto } from "$app/navigation";
  import CrownIcon from "@lucide/svelte/icons/crown";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ShieldIcon from "@lucide/svelte/icons/shield";
  import UserMinusIcon from "@lucide/svelte/icons/user-minus";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";

  import { app } from "#lib/app.svelte.js";
  import type { RoomMembers, RoomUser } from "#lib/chat/rooms/room.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatCount, formatSpeed, plural } from "#lib/format.js";
  import { freezesWhileUsed, heldOrder, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepsScroll } from "#lib/lists/scroll.svelte.js";
  import { userHref } from "#lib/nav.js";
  import type { Outcome } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  // who is in a room: operators then members, online first, a filter and the counts; in a private
  // room its owner and operators also manage who belongs from here
  let { members }: { members: RoomMembers } = $props();

  type Line =
    | { kind: "header"; key: string; title: string; names: string[] }
    | { kind: "user"; key: string; user: RoomUser; names: string[] };

  let typed = $state("");
  let adding = $state("");
  /** the row whose menu is open, which holds the list still */
  let menuFor = $state<string | null>(null);

  const PRESENCE = { Online: 0, Away: 1, Offline: 2 } as const;
  const blocked = (name: string) => !!users.get(name)?.block;
  const order = (a: RoomUser, b: RoomUser) =>
    Number(blocked(a.username)) - Number(blocked(b.username)) ||
    PRESENCE[a.status] - PRESENCE[b.status] ||
    a.username.localeCompare(b.username);

  const all = $derived(members.users.rows);
  const online = $derived(all.filter((u) => u.status === "Online").length);
  const us = $derived(app.state?.user.username ?? "");
  const role = $derived(session.can("ReadWrite") ? members.roleOf(us) : null);
  const room = $derived(members.room);
  // the list holds still while it is pressed, selected in or a name's card or menu is open from it
  // svelte-ignore state_referenced_locally
  const hold = heldOrder<Line>((l) => l.key, members.users.freeze);
  $effect(() => (menuFor ? interaction.hold() : undefined));
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

  /** What the owner or an operator may do to `name` in a private room */
  const powers = (name: string) => {
    if (!role || name === us || name === room?.owner) return null;
    const operator = !!room?.operators?.includes(name);
    return {
      // operators take out members only; the owner takes out operators too
      remove: role === "owner" || !operator,
      promote: role === "owner" && !operator,
      demote: role === "owner" && operator
    };
  };

  const report = (done: string) => (outcome: Outcome<unknown>) =>
    toast(outcome.ok ? done : outcome.error, outcome.ok ? "info" : "error");

  const add = async () => {
    const name = adding.trim();
    if (!name) return;
    const outcome = await members.addMember(name);
    report(`${name} was added to ${members.name}`)(outcome);
    if (outcome.ok) adding = "";
  };
  const remove = (name: string) =>
    confirmation.ask({
      title: "Remove from room",
      body: `Remove ${name} from ${members.name}? They can rejoin only if added again.`,
      confirm: "Remove",
      run: () => void members.removeMember(name).then(report(`${name} was removed`))
    });
  const giveUp = () =>
    confirmation.ask({
      title: "Give up ownership",
      body: `Give up ${members.name}? It becomes a public room with no owner and you leave it. Its members lose access without being told.`,
      confirm: "Give up",
      run: () => void members.dropOwnership().then(report(`Gave up ${members.name} and left it`))
    });
  const quit = () =>
    confirmation.ask({
      title: "Leave private room",
      body: `Leave ${members.name}? You can rejoin only if added again.`,
      confirm: "Leave",
      run: () => void members.dropMembership().then(report(`Left ${members.name}`))
    });
</script>

<aside class="@container flex min-h-0 w-full flex-col gap-2 p-2" aria-label="Room users">
  <div class="flex items-center gap-1 px-1">
    <p class="text-xs text-fg-muted">
      {plural(all.length, "user", "users")} · {formatCount(online)} online
    </p>
    {#if room?.isPrivate && session.can("ReadWrite")}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              class="ml-auto"
              aria-label="Private room menu"><EllipsisIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="end" class="min-w-52">
          <Menu.Label>
            {#if role === "owner"}Owner of this room{:else if role === "operator"}Operator of this
              room{:else}Member of this room{/if}
          </Menu.Label>
          {#if role === "owner"}
            <Menu.Item onSelect={giveUp}><CrownIcon />Give up ownership</Menu.Item>
          {:else}
            <Menu.Item onSelect={quit}><UserMinusIcon />Leave private room</Menu.Item>
          {/if}
        </Menu.Content>
      </Menu.Root>
    {/if}
  </div>
  {#if members.error}
    <Notice kind="error">
      {members.error}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => void members.load()}>Retry</Button>
      {/snippet}
    </Notice>
  {/if}
  <Input
    type="search"
    class="h-control-sm"
    placeholder="Filter"
    aria-label="Filter users"
    bind:value={typed} />
  {#if role}
    <form
      class="flex gap-1"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}>
      <Input
        class="h-control-sm"
        placeholder="Add member"
        aria-label="Username to add"
        bind:value={adding} />
      <Tooltip text="Add">
        {#snippet child({ props })}
          <Button
            {...props}
            type="submit"
            size="icon-sm"
            variant="outline"
            aria-label="Add"
            disabled={!adding.trim()}><UserPlusIcon /></Button>
        {/snippet}
      </Tooltip>
    </form>
  {/if}
  <div
    class="min-h-0 flex-1 overflow-y-auto rounded-control outline-none focus-visible:ring-2 focus-visible:ring-accent pointer-fine:[--line-h:1.625rem]"
    role="grid"
    aria-label="Room members"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed(members.users.freeze)}
    {@attach keepsScroll(`chat.members.${members.name}`)}>
    {#each lines as line, i (line.key)}
      {#if line.kind === "header"}
        <div
          role="row"
          {...list.line(line, i)}
          class="px-1 pt-2 pb-0.5 text-xs font-medium text-fg-muted">
          <span role="columnheader">{line.title}</span>
        </div>
      {:else}
        {@const u = line.user}
        {@const can = powers(u.username)}
        <div
          role="row"
          {...list.line(line, i)}
          class={[
            "group flex items-center gap-1.5 pick-row rounded-badge px-1 py-0.5 text-sm",
            members.users.removed.has(u.username) && "opacity-40"
          ]}>
          <span role="none" class="name-stats flex-1">
            <span role="gridcell" class="flex items-center gap-1.5">
              <Presence presence={u.status} />
              <!-- the slot stays without a flag, so the names line up -->
              <span class="flex w-5 shrink-0 justify-center"><Flag code={u.countryCode} /></span>
              <span class={["min-w-0 truncate", blocked(u.username) && "line-through"]}>
                <Username name={u.username} card />
              </span>
              {#if u.username === room?.owner}<CrownIcon
                  class="size-3 shrink-0 text-warning-icon"
                  aria-label="Owner" />{/if}
              {#if blocked(u.username)}<Tooltip text="Blocked" dense spoken
                  >{#snippet child({ props })}<span
                      {...props}
                      class="size-2 shrink-0 rounded-full bg-danger"></span
                    >{/snippet}</Tooltip
                >{/if}
              {#if u.self}<span class="text-xs text-accent">you</span>{/if}
            </span>
            <!-- a narrow panel gives the names its width; the card on a name has the figures -->
            <span
              role="gridcell"
              class="text-right text-xs text-fg-muted tabular-nums @max-[22rem]:hidden"
              >{formatSpeed(u.averageSpeed)} · {plural(u.fileCount, "file", "files")}</span>
          </span>
          {#if can}
            <Menu.Root
              open={menuFor === u.username}
              onOpenChange={(o) => (menuFor = o ? u.username : null)}>
              <Menu.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    class="shrink-0 row-action"
                    aria-label={`Manage ${u.username}`}><EllipsisIcon /></Button>
                {/snippet}
              </Menu.Trigger>
              <Menu.Content align="end" class="min-w-48">
                {#if can.promote}
                  <Menu.Item
                    onSelect={() =>
                      void members
                        .addOperator(u.username)
                        .then(report(`${u.username} is now an operator`))}
                    ><ShieldIcon />Make operator</Menu.Item>
                {/if}
                {#if can.demote}
                  <Menu.Item
                    onSelect={() =>
                      void members
                        .removeOperator(u.username)
                        .then(report(`${u.username} is no longer an operator`))}
                    ><ShieldIcon />Remove as operator</Menu.Item>
                {/if}
                {#if can.remove}
                  <Menu.Item onSelect={() => remove(u.username)}
                    ><UserMinusIcon />Remove from room</Menu.Item>
                {/if}
              </Menu.Content>
            </Menu.Root>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
</aside>

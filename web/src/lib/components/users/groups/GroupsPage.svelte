<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import GroupDot from "#lib/components/users/GroupDot.svelte";
  import { valueAt } from "#lib/config/sections.js";
  import { adminView } from "#lib/config/view.svelte.js";
  import { editabilityOf, writeFromPage } from "#lib/config/write.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { undoable } from "#lib/feedback/undo.js";
  import { listText, plural } from "#lib/format.js";
  import { RowDrag } from "#lib/lists/rowdrag.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call, type Outcome } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import {
    BUDDIES,
    deletePatch,
    DEFAULT,
    groupLabel,
    type GroupSummary,
    LEECHERS,
    movedIn,
    ORDER,
    type Upload,
    settingsText,
    USER_DEFINED,
    VISIBLE_TO,
    type VisibleTo,
    withHidden
  } from "#lib/users/groups.js";
  import { NOT_CONFIGURED } from "#lib/wording.js";

  import GroupEditor from "./GroupEditor.svelte";

  const PRIVILEGED = "privileged";
  /** how long after the last move the order is written, so several moves are one write */
  const REORDER_PAUSE = 600;

  // every group in the order it goes, the privileged first, then the names of groups the
  // configuration no longer has that still have members. Which groups there are, their order and
  // their settings are options, changed here by an administrator; members of a group that is gone
  // can be moved or let go by anyone who may write
  const list = new Loader(() => call(() => api.GET("/api/v0/groups")));
  keepCurrent(
    () => void list.load(),
    () => "users"
  );
  void list.load();
  $effect(() => live.on("application.options", () => void list.load()));

  const admin = $derived(session.can("Administrator"));
  const write = $derived(session.can("ReadWrite"));
  const view = adminView();
  const shares = new Loader(() => call(() => api.GET("/api/v0/shares")));
  $effect(() => {
    if (admin) untrack(() => void shares.load());
  });

  /** groups an Undo still waits on, kept out of the list until it goes */
  const leaving = new SvelteSet<string>();
  const groups = $derived((list.data?.groups ?? []).filter((g) => !leaving.has(g.name)));
  const defined = $derived(groups.filter((g) => g.defined));
  const gone = $derived(groups.filter((g) => !g.defined));
  /** the user-defined groups of the configuration, which members can be moved to */
  const targets = $derived(defined.filter((g) => !g.builtin));
  const privileged = $derived(defined.filter((g) => g.name === PRIVILEGED));
  const everyone = $derived(defined.filter((g) => g.name === DEFAULT));

  /** the order a move asked for that is not saved yet, null while the daemon's is shown */
  let reordered = $state<string[] | null>(null);
  /** the groups that go in the order, as it is shown; one made since a move goes last */
  const ranked = $derived.by(() => {
    const rest = defined.filter((g) => g.name !== PRIVILEGED && g.name !== DEFAULT);
    if (!reordered) return rest;
    const held = reordered;
    return [
      ...held.flatMap((name) => rest.find((g) => g.name === name) ?? []),
      ...rest.filter((g) => !held.includes(g.name))
    ];
  });
  const names = $derived(ranked.map((g) => g.name));

  const visibleTo = $derived((valueAt(view.data?.values, VISIBLE_TO) ?? {}) as VisibleTo);
  /** the share aliases a group can be shown: every share's, and any the option still names */
  const aliases = $derived.by(() => {
    const found = Object.values(shares.data ?? {})
      .flat()
      .filter((s) => !s.isExcluded)
      .map((s) => s.alias);
    const all = [...found, ...Object.keys(visibleTo)];
    return all.filter((a, i) => all.findIndex((b) => b.toLowerCase() === a.toLowerCase()) === i);
  });

  /** what the options let an administrator do here, null while they can't be read */
  const edit = $derived(
    view.data && admin ? editabilityOf(view.data, [USER_DEFINED, VISIBLE_TO]) : null
  );
  const editable = $derived(!!edit && edit.mode !== "locked");
  const orderable = $derived(
    !!view.data && admin && editabilityOf(view.data, [ORDER]).mode !== "locked"
  );
  // an option's editability has no notice while remote configuration is off, so this gives one
  const notice = $derived(
    view.data && valueAt(view.data.values, "remote_configuration") !== true
      ? "Read-only because remote configuration is off"
      : (edit?.notice ?? null)
  );

  /** the group the editor is open for, "new" for a group not made yet */
  let editing = $state<GroupSummary | "new" | null>(null);

  /** a group's upload settings in the options in effect */
  const uploadOf = (g: GroupSummary) =>
    (valueAt(
      app.options,
      g.builtin ? `transfers.groups.${g.name}.upload` : `${USER_DEFINED}.${g.name}.upload`
    ) ?? null) as Upload | null;
  /** what a group sets, in a few words */
  const uploads = (g: GroupSummary) => {
    // the server's privileged users go before every group, with every slot and no limits
    if (g.name === PRIVILEGED) return "Served first";
    const set = settingsText(uploadOf(g));
    if (set.length) return set.join(" · ");
    return g.name === DEFAULT ? "Global settings" : "Sets nothing";
  };
  /** who is in a smart group, in words */
  const who = (g: GroupSummary) => {
    if (g.name === PRIVILEGED) return "Privileged on the server";
    if (g.name === LEECHERS) {
      const files = valueAt(app.options, "transfers.groups.leechers.thresholds.files");
      const folders = valueAt(app.options, "transfers.groups.leechers.thresholds.directories");
      return typeof files === "number" && typeof folders === "number"
        ? `Share fewer than ${plural(files, "file", "files")} or ${plural(folders, "folder", "folders")}`
        : "Share too little";
    }
    return "Holds what no group above sets";
  };

  /** Writes the options without the group `name`, its shares then seen by none of it */
  const remove = async (name: string): Promise<Outcome<null>> => {
    const data = view.data;
    if (!data) return { ok: false, error: "Settings are still loading", status: 0 };
    return writeFromPage(data, deletePatch(data.values, name).set);
  };

  /**
   * Shows `name` gone at once, with an Undo, and runs `commit` once the Undo is past; a commit
   * that fails brings the group back
   */
  const later = (name: string, text: string, commit: () => Promise<Outcome<unknown>>) => {
    leaving.add(name);
    undoable(
      text,
      async () => {
        const outcome = await commit();
        if (!outcome.ok) toast(outcome.error, "error");
        await list.load();
        leaving.delete(name);
      },
      () => leaving.delete(name)
    );
  };

  /** Makes a group of the configuration under a name its members still hold */
  const make = async (name: string) => {
    const data = view.data;
    if (!data) return;
    const groups = (valueAt(data.values, USER_DEFINED) ?? {}) as Record<string, unknown>;
    const outcome = await writeFromPage(data, { [USER_DEFINED]: { ...groups, [name]: {} } });
    if (!outcome.ok) return toast(outcome.error, "error");
    toast(`Group ${name} created`);
    void list.load();
  };

  const move = (from: string, to: string) =>
    later(from, `Members of ${from} moved to ${to}`, () =>
      call(() =>
        api.POST("/api/v0/groups/{name}/members/move", {
          params: { path: { name: from } },
          body: { to }
        })
      )
    );

  const letGo = (name: string) =>
    later(name, `Members of ${name} removed`, () =>
      call(() => api.DELETE("/api/v0/groups/{name}/members", { params: { path: { name } } }))
    );

  /** the order's writes, one at a time, each after the view the one before it changed */
  let writing: Promise<void> = Promise.resolve();
  /** the write a move is waiting to make, undefined while none waits */
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Writes the order shown, now rather than when the pause after the last move ends */
  const flush = () => {
    clearTimeout(timer);
    timer = undefined;
    writing = writing.then(async () => {
      const data = view.data;
      if (!data || !reordered) return;
      const sent = reordered;
      const outcome = await writeFromPage(data, { [ORDER]: sent });
      if (!outcome.ok) toast(`Could not save the order: ${outcome.error}`, "error");
      await Promise.all([view.load(), list.load()]);
      if (reordered === sent || !outcome.ok) reordered = null;
    });
  };
  /** Shows `order` at once and writes it a moment after the last move */
  const reorder = (order: string[]) => {
    reordered = order;
    clearTimeout(timer);
    timer = setTimeout(flush, REORDER_PAUSE);
  };
  // leaving the page writes a move still waiting, so the order shown is the order kept
  $effect(() => () => {
    if (timer !== undefined) flush();
  });
  const STEPS = [
    { by: -1, label: "up", Icon: ArrowUpIcon },
    { by: 1, label: "down", Icon: ArrowDownIcon }
  ];
  const step = (at: number, by: number) => reorder(movedIn(names, at, at + by));

  let box = $state<HTMLElement | null>(null);
  const drag = new RowDrag(
    () => box,
    (key, before) => {
      const others = names.filter((n) => n !== key);
      reorder(movedIn(names, names.indexOf(key), before ? others.indexOf(before) : others.length));
    }
  );

  const saved = () => {
    void view.load();
    void list.load();
  };
</script>

{#snippet newGroup()}
  <Button size="sm" variant="outline" onclick={() => (editing = "new")}
    ><PlusIcon />New group</Button>
{/snippet}

{#snippet row(g: GroupSummary, at: number | null)}
  {@const movable = at !== null && orderable}
  <li
    data-line={g.name}
    style:translate={drag.shift(g.name)}
    class={[
      "relative flex flex-wrap items-center gap-x-4 gap-y-2 bg-surface py-3 pr-5",
      movable ? "pl-8" : "pl-5",
      drag.key === g.name && "z-10 shadow-raised",
      drag.key !== null &&
        drag.key !== g.name &&
        "transition-[translate] duration-(--motion-fast) ease-(--ease-out-soft)"
    ]}>
    {#if movable}
      <button
        type="button"
        tabindex="-1"
        aria-hidden="true"
        class="absolute inset-y-0 left-0 flex w-8 cursor-grab touch-none items-center justify-center text-fg-subtle hover:text-fg-muted active:cursor-grabbing"
        onpointerdowncapture={(e) => drag.press(e, g.name, names)}
        ><GripVerticalIcon class="size-4" /></button>
    {/if}
    <div class="flex min-w-0 flex-1 flex-col gap-0.5">
      <div class="flex min-w-0 items-center gap-2">
        <GroupDot color={g.color} />
        <span class={["min-w-0 truncate text-sm font-medium", g.name !== DEFAULT && "font-mono"]}
          >{groupLabel(g.name)}</span>
        <a
          href={href(`/users?group=${encodeURIComponent(g.name)}`)}
          class="shrink-0 text-xs text-accent tabular-nums hover:underline"
          >{plural(g.members, "member", "members")}</a>
      </div>
      <p class="text-xs text-fg-muted">
        {[
          g.kind === "smart" ? who(g) : g.name === BUDDIES ? "Your buddy list" : null,
          g.defined && uploads(g),
          g.shares.length ? `Sees ${listText(g.shares)}` : null
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
    </div>
    <div class="flex shrink-0 items-center gap-1">
      {#if !g.defined}
        {#if admin && editable}
          <Button size="sm" variant="outline" onclick={() => void make(g.name)}
            >Create group</Button>
        {/if}
        {#if write && targets.length}
          <Menu.Root>
            <Menu.Trigger>
              {#snippet child({ props })}
                <Button {...props} size="sm" variant="outline"
                  >Move members<ChevronDownIcon /></Button>
              {/snippet}
            </Menu.Trigger>
            <Menu.Content align="end">
              {#each targets as t (t.name)}
                <Menu.Item onSelect={() => move(g.name, t.name)}
                  ><GroupDot color={t.color} />{t.name}</Menu.Item>
              {/each}
            </Menu.Content>
          </Menu.Root>
        {/if}
        {#if write}
          <Button size="sm" variant="ghost" onclick={() => letGo(g.name)}>Remove members</Button>
        {/if}
      {:else}
        {#if movable}
          {#each STEPS as dir (dir.label)}
            <Tooltip text={`Move ${dir.label}`}>
              {#snippet child({ props })}
                <Button
                  {...props}
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Move ${g.name} ${dir.label}`}
                  disabled={at + dir.by < 0 || at + dir.by >= names.length}
                  onclick={() => step(at, dir.by)}><dir.Icon /></Button>
              {/snippet}
            </Tooltip>
          {/each}
        {/if}
        {#if editable && g.name !== PRIVILEGED}
          <Button size="sm" variant="outline" onclick={() => (editing = g)}>Edit</Button>
        {/if}
        {#if editable && !g.builtin}
          <Tooltip text="Delete group">
            {#snippet child({ props })}
              <Button
                {...props}
                size="icon-sm"
                variant="ghost"
                aria-label={`Delete group ${g.name}`}
                onclick={() =>
                  later(
                    g.name,
                    withHidden(
                      `Group ${g.name} deleted`,
                      view.data ? deletePatch(view.data.values, g.name).hidden : []
                    ),
                    () => remove(g.name)
                  )}><Trash2Icon /></Button>
            {/snippet}
          </Tooltip>
        {/if}
      {/if}
    </div>
  </li>
{/snippet}

<div class="flex flex-wrap items-center gap-inline">
  {#if editable}{@render newGroup()}{/if}
  {#if list.data}
    <span class="ml-auto text-sm text-fg-muted">{plural(groups.length, "group", "groups")}</span>
  {/if}
</div>

{#if admin && notice}<Notice kind="info">{notice}</Notice>{/if}
<LoadError loader={list} />

{#if list.loading}
  <Skeleton class="h-64" />
{:else}
  <Card
    title="Groups"
    description="Each setting comes from the highest group that sets it, and higher groups are served first">
    {#snippet flush()}
      <ul class="divide-y border-t" bind:this={box}>
        {#each privileged as g (g.name)}{@render row(g, null)}{/each}
        {#each ranked as g, i (g.name)}{@render row(g, i)}{/each}
        {#each everyone as g (g.name)}{@render row(g, null)}{/each}
      </ul>
      {#if !targets.length}
        <p class="border-t px-5 py-3 text-sm text-fg-muted">No groups of your own yet</p>
      {/if}
    {/snippet}
  </Card>

  {#if gone.length}
    <Card title={NOT_CONFIGURED} description="Groups that no longer exist but still have members">
      {#snippet flush()}
        <ul class="divide-y border-t">
          {#each gone as g (g.name)}{@render row(g, null)}{/each}
        </ul>
      {/snippet}
    </Card>
  {/if}
{/if}

{#if editing && view.data}
  <!-- a group whose delete still waits on its Undo keeps its name taken -->
  <GroupEditor
    group={editing === "new" ? null : editing}
    view={view.data}
    {aliases}
    taken={(list.data?.groups ?? []).filter((g) => g.defined).map((g) => g.name)}
    onclose={() => (editing = null)}
    onsaved={saved} />
{/if}

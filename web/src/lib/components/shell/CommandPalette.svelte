<script lang="ts">
  import { goto } from "$app/navigation";
  import { Command, Dialog } from "bits-ui";

  import { activityPrefs } from "#lib/activity/activity.svelte.js";
  import { screenHref } from "#lib/analysis/slots.svelte.js";
  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import { sections } from "#lib/config/sections.js";
  import {
    href,
    navItems,
    profileItem,
    roomHref,
    systemItem,
    systemTabs,
    userHref
  } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { recent } from "#lib/search/recent.svelte.js";
  import { openSearch, searchFor } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { paletteScore, paletteValue } from "#lib/shell/palette.js";
  import { palette } from "#lib/shell/palette.svelte.js";
  import { opensPalette, shortcutText, TYPING_PAUSE } from "#lib/ui/keys.js";
  import { reach, USER_ACTIONS } from "#lib/users/actions.js";

  /** One thing the palette can do: go to a screen, a user, a room, a setting, a search */
  type PaletteEntry = {
    id: string;
    /** the heading it is listed under */
    group: string;
    label: string;
    /** muted text after the label */
    hint?: string;
    /** more words it is found by, such as a setting's key */
    keywords?: string;
    run: () => void;
  };

  let query = $state("");
  let known = $state.raw<string[]>([]);

  const go = (to: string) => () => void goto(href(to));

  const search = (text: string) => () => openSearch(text);

  const fixed = $derived.by((): PaletteEntry[] => {
    const agent = app.mode === "agent";
    const screens = agent ? [] : [...navItems, profileItem];
    const entries: PaletteEntry[] = [
      ...screens.map((s) => ({
        id: s.path,
        group: "Screens",
        label: s.title,
        run: () => void goto(screenHref(s.path))
      })),
      { id: systemItem.path, group: "Screens", label: "System", run: go(systemItem.path) },
      ...(agent
        ? []
        : [
            {
              id: "activity",
              group: "Screens",
              label: activityPrefs.open ? "Close the activity log" : "Activity log",
              hint: shortcutText("J"),
              keywords: "browsing searches queued uploads log network",
              run: () => (activityPrefs.open = !activityPrefs.open)
            }
          ]),
      ...systemTabs.map((t) => ({
        id: `/system/${t.tab}`,
        group: "System",
        label: t.title,
        hint: "System",
        run: go(`/system/${t.tab}`)
      }))
    ];
    if (session.can("Administrator")) {
      for (const section of sections) {
        entries.push({
          id: `setting:${section.id}`,
          group: "Settings",
          label: section.title,
          hint: "Settings",
          keywords: section.fields.map((f) => `${f.label} ${f.key}`).join(" "),
          run: go(`/system/settings?section=${section.id}`)
        });
      }
    }
    if (agent) return entries;
    for (const text of [...recent.pinned, ...recent.texts]) {
      entries.push({
        id: `search:${text}`,
        group: "Searches",
        label: text,
        hint: "Search again",
        run: search(text)
      });
    }
    for (const room of app.state?.rooms ?? []) {
      entries.push({
        id: `room:${room}`,
        group: "Rooms",
        label: room,
        run: () => goto(roomHref(room))
      });
    }
    const names = new Set([...chat.conversations.rows.map((c) => c.username), ...known]);
    for (const name of names) {
      entries.push({
        id: `user:${name}`,
        group: "Users",
        label: name,
        hint: USER_ACTIONS.profile.label,
        run: () => goto(userHref(name))
      });
    }
    return entries;
  });

  /** What the typed text can be: a search, or a user to reach */
  const typed = $derived.by((): PaletteEntry[] => {
    const text = query.trim();
    if (!text || app.mode === "agent") return [];
    const write = session.can("ReadWrite");
    return [
      ...(write && app.isServerConnected
        ? [{ id: "do:network", group: "Actions", label: searchFor(text), run: search(text) }]
        : []),
      ...reach(text).map((action) => ({
        id: `do:${action.id}`,
        group: "Actions",
        label: action.of(text),
        run: () => goto(action.href)
      }))
    ];
  });

  // grouped in the order the groups first come; the command list filters and ranks within them
  const groups = $derived.by(() => {
    const byGroup = new Map<string, PaletteEntry[]>();
    for (const entry of [...fixed, ...typed]) {
      const list = byGroup.get(entry.group);
      if (list) list.push(entry);
      else byGroup.set(entry.group, [entry]);
    }
    return [...byGroup];
  });

  // users the directory knows by what was typed, a moment after typing stops
  $effect(() => {
    const text = query.trim();
    if (!palette.open || text.length < 2 || app.mode === "agent") {
      known = [];
      return;
    }
    const timer = setTimeout(async () => {
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { q: text, limit: 8 } } })
      );
      // a directory that can't be read leaves the users already listed
      if (outcome.ok && query.trim() === text) known = outcome.data.users.map((u) => u.username);
    }, TYPING_PAUSE);
    return () => clearTimeout(timer);
  });

  const run = (entry: PaletteEntry) => {
    palette.open = false;
    query = "";
    entry.run();
  };
</script>

<svelte:window
  onkeydown={(e) => {
    if (!opensPalette(e)) return;
    e.preventDefault();
    palette.toggle();
  }} />

<Dialog.Root bind:open={palette.open} onOpenChange={(open) => !open && (query = "")}>
  <Dialog.Portal>
    <Scrim />
    <Dialog.Content
      class="fixed top-[15svh] left-1/2 z-50 flex max-h-[70svh] w-[min(36rem,calc(100vw-2rem))] -translate-x-1/2 flex-col overflow-hidden rounded-card border bg-overlay shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <Dialog.Title class="sr-only">Go to</Dialog.Title>
      <Command.Root loop class="flex min-h-0 flex-col" filter={paletteScore}>
        <Command.Input
          bind:value={query}
          aria-label="Go to a screen, user, room or setting, or search"
          placeholder="Go to a screen, user, room or setting, or search"
          class="h-12 w-full shrink-0 border-b bg-transparent px-4 text-base outline-none placeholder:text-fg-muted" />
        <Command.List class="min-h-0 overflow-y-auto p-1.5">
          <Command.Viewport>
            <Command.Empty class="px-3 py-6 text-center text-sm text-fg-muted">
              No matches
            </Command.Empty>
            {#each groups as [group, entries] (group)}
              <Command.Group value={group}>
                <Command.GroupHeading class="px-2.5 pt-2 pb-1 text-xs font-medium text-fg-muted">
                  {group}
                </Command.GroupHeading>
                <Command.GroupItems>
                  {#each entries as entry (entry.id)}
                    <Command.Item
                      value={paletteValue(entry.label, entry.id)}
                      keywords={[entry.hint ?? "", entry.keywords ?? ""]}
                      onSelect={() => run(entry)}
                      class="flex h-9 items-center gap-2 rounded-control px-2.5 text-sm data-selected:bg-hover">
                      <span class="truncate">{entry.label}</span>
                      {#if entry.hint}
                        <span class="ml-auto shrink-0 text-xs text-fg-subtle">{entry.hint}</span>
                      {/if}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            {/each}
          </Command.Viewport>
        </Command.List>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<script lang="ts">
  import { goto } from "$app/navigation";
  import SearchIcon from "@lucide/svelte/icons/search";
  import UserSearchIcon from "@lucide/svelte/icons/user-search";
  import { Combobox } from "bits-ui";
  import { onDestroy } from "svelte";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Input from "#lib/components/ui/Input.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, dateTime } from "#lib/format.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { profileOf } from "#lib/users/actions.js";
  import { nameColor } from "#lib/users/colors.js";
  import { bestMatches } from "#lib/users/match.js";
  import type { UserSummary } from "#lib/users/users.svelte.js";

  import GroupBadge from "./GroupBadge.svelte";
  import Presence from "./Presence.svelte";

  type Props = {
    /** the text a moment after typing stops, which the list under the box filters by */
    onsearch: (text: string) => void;
  };

  let { onsearch }: Props = $props();

  /** how many users are read for each text, the closest of them are listed */
  const CANDIDATES = 100;
  const LISTED = 8;

  let typed = $state("");
  let open = $state(false);
  /** what the dropdown reports as chosen, emptied at once so the same entry can be chosen again */
  let chosen = $state("");
  /** the users the directory matched for the text; tags and notes match too */
  let found = $state.raw<UserSummary[]>([]);
  /** the name being asked of the network */
  let looking = $state<string | null>(null);

  // no username has spaces around it, so what is around the text is no part of the name
  const text = $derived(typed.trim());
  const matches = $derived(bestMatches(found, text).slice(0, LISTED));
  const unknown = $derived(!found.some((u) => u.username === text));
  const asking = $derived(looking === text);

  $effect(() => {
    const value = text;
    const timer = setTimeout(async () => {
      onsearch(value);
      if (!value) {
        found = [];
        return;
      }
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { q: value, limit: CANDIDATES } } })
      );
      // a directory that can't be read leaves the users already listed
      if (outcome.ok && text === value) found = outcome.data.users;
    }, 200);
    return () => clearTimeout(timer);
  });

  /** Forgets the look-up running, whose answer then opens nothing */
  const abandon = () => {
    looking = null;
  };
  onDestroy(abandon);

  const lookUp = async (name: string) => {
    looking = name;
    const outcome = await profileOf(name, true);
    if (looking !== name) return;
    looking = null;
    if (text !== name) return;
    if (outcome.ok) void goto(userHref(name));
    else {
      open = true;
      toast(outcome.status === 404 ? `No user named ${name} was found.` : outcome.error, "error");
    }
  };

  const choose = (name: string) => {
    chosen = "";
    if (found.some((u) => u.username === name)) void goto(userHref(name));
    else void lookUp(name);
  };
</script>

<!-- a look-up holds the dropdown open, where its entry shows it running -->
<Combobox.Root
  type="single"
  bind:open={() => (open || asking) && !!text, (on) => (open = on)}
  bind:value={chosen}
  onValueChange={choose}>
  <Combobox.Input oninput={() => (open = true)} onclick={() => (open = true)}>
    {#snippet child({ props })}
      <Input
        {...props}
        bind:value={typed}
        icon={SearchIcon}
        wrapperClass="w-full max-w-xs"
        type="search"
        autocomplete="off"
        placeholder="Username, tag or note"
        aria-label="Search users" />
    {/snippet}
  </Combobox.Input>
  <Combobox.Portal>
    <Combobox.Content
      sideOffset={4}
      onEscapeKeydown={abandon}
      onInteractOutside={abandon}
      class="z-50 max-h-(--bits-combobox-content-available-height) w-(--bits-combobox-anchor-width) origin-(--bits-combobox-content-transform-origin) rounded-control border bg-overlay p-1 text-fg shadow-raised data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <Combobox.Viewport>
        <!-- entries made anew move the highlight to the first, the closest match, when the users
             arrive; otherwise it stays on the look-up entry that was alone before them -->
        {#key found}
          {#each matches as u (u.username)}
            <Combobox.Item
              value={u.username}
              label={u.username}
              class="flex flex-col gap-0.5 rounded-badge px-2 py-1 text-sm data-highlighted:bg-hover">
              <span class="flex items-center gap-2">
                <span
                  class="min-w-0 flex-1 truncate font-medium"
                  style:color={nameColor(u.username)}>{u.username}</span>
                <span class="flex shrink-0 items-center gap-1.5 text-xs">
                  <Presence presence={u.presence} />{u.presence ?? "Unknown"}
                </span>
              </span>
              <span class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-fg-muted">
                <GroupBadge group={u.group} class="max-w-full truncate" />
                <span
                  class="min-w-0 truncate"
                  title={u.lastSeenAt ? dateTime(u.lastSeenAt) : undefined}>
                  {u.lastSeenAt ? `Last seen ${ago(u.lastSeenAt, clock.now)}` : "Never seen"}
                </span>
              </span>
            </Combobox.Item>
          {/each}
          {#if unknown}
            {#if matches.length}<Combobox.Separator class="-mx-1 my-1 h-px bg-border" />{/if}
            <Combobox.Item
              value={text}
              label={text}
              disabled={asking || !app.isServerConnected}
              class="flex flex-col gap-0.5 rounded-badge px-2 py-1.5 text-sm data-highlighted:bg-hover">
              <span class="flex items-center gap-2">
                {#if asking}
                  <Spinner class="size-4 shrink-0" label={`Looking up ${text}`} />
                {:else}
                  <UserSearchIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
                {/if}
                <span class="min-w-0 truncate">{asking ? "Looking up" : "Look up"} {text}</span>
              </span>
              {#if !app.isServerConnected}
                <span class="text-xs text-fg-muted">Connect to the Soulseek server first</span>
              {/if}
            </Combobox.Item>
          {/if}
        {/key}
      </Combobox.Viewport>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>

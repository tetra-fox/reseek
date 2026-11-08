<script lang="ts">
  import { afterNavigate, goto } from "$app/navigation";
  import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
  import BellOffIcon from "@lucide/svelte/icons/bell-off";
  import BellRingIcon from "@lucide/svelte/icons/bell-ring";
  import MessagesSquareIcon from "@lucide/svelte/icons/messages-square";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import { Dialog } from "bits-ui";

  import { paneKey } from "#lib/chat/chat.svelte.js";
  import { columns } from "#lib/chat/columns.svelte.js";
  import { chatName } from "#lib/chat/notices.js";
  import { chatWantsDesktop, notifier, notifyOptions } from "#lib/chat/notify.svelte.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import { chatLayout } from "#lib/chat/prefs.svelte.js";
  import { chatOrder } from "#lib/chat/rooms/order.js";
  import { resultsHref } from "#lib/chat/route.js";
  import type { SearchKind } from "#lib/chat/search/match.js";
  import { ChatSearch } from "#lib/chat/search/search.svelte.js";
  import JoinRoomDialog from "#lib/components/chat/rooms/JoinRoomDialog.svelte";
  import PublicFeed from "#lib/components/chat/rooms/PublicFeed.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import ResizeHandle from "#lib/components/ui/ResizeHandle.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { chatHref, roomHref } from "#lib/nav.js";
  import { useCrumbs } from "#lib/shell/crumbs.svelte.js";
  import { layerOpen } from "#lib/ui/keys.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { DESKTOP_NEEDS_HTTPS, TURN_ON_NOTIFICATIONS } from "#lib/wording.js";

  import ChatList from "./ChatList.svelte";
  import NewMessageDialog from "./NewMessageDialog.svelte";
  import Pane from "./Pane.svelte";
  import SearchResults from "./search/SearchResults.svelte";

  type Props = {
    /** the pane to show, "search" for the search results, "public" for the public room feed,
     * null for none */
    target: Target | "search" | "public" | null;
    /** a message to open the pane at */
    at: number | null;
    /** the search, on its results and on a chat opened from them */
    q: string;
    /** the messages the search looks at */
    kind: SearchKind;
  };

  let { target, at, q, kind }: Props = $props();

  /** how many messages one page of the search results holds */
  const RESULTS_PAGE = 50;

  $effect(() => {
    columns.listShown = viewport.lg;
  });
  let sheet = $state(false);
  let joining = $state(false);
  let joinName = $state("");
  let writing = $state(false);
  let writeTo = $state("");

  /** kept while the screen is open, so coming back from a chat finds the results as they were */
  const results = new ChatSearch(
    () => q,
    () => kind,
    RESULTS_PAGE
  );
  const fromResults = $derived(!!q && !!target && target !== "search" && target !== "public");
  // the results wait only while the search is what is being followed
  $effect(() => {
    if (!q) results.forget();
  });

  const current = $derived(
    target && target !== "search" && target !== "public" ? paneKey(target.kind, target.name) : null
  );
  const wantsDesktop = $derived(chatWantsDesktop(notifyOptions()));
  const asking = $derived(notifier.permission === "default" && wantsDesktop);
  const insecure = $derived(notifier.permission === "insecure" && wantsDesktop);

  // the breadcrumb names the chat on show
  useCrumbs(() => {
    if (target === "search") return [{ label: "Search results" }];
    if (target === "public") return [{ label: "All public rooms" }];
    if (!target) return [];
    return [{ label: target.kind === "room" ? `#${target.name}` : chatName(target) }];
  });

  // a narrow screen shows the list in a sheet, which going to a chat closes
  afterNavigate(() => (sheet = false));

  const onjoin = (name = "") => {
    joinName = name;
    joining = true;
  };
  const onnew = (name = "") => {
    writeTo = name;
    writing = true;
  };

  // focus stays where it was, so the keys go on working
  const go = (t: Target) =>
    void goto(t.kind === "user" ? chatHref(t.name) : roomHref(t.name), { reset: false });

  // the chat opened from the results gives way to them, so Back passes over neither twice
  const backToResults = () => void goto(resultsHref(q, kind), { replace: true });

  // Alt+Up and Alt+Down move to the chat above or below; Escape goes back to the search results
  // a chat was opened from, once nothing open takes it
  const keydown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && fromResults && !e.defaultPrevented && !layerOpen()) {
      backToResults();
      return;
    }
    if (
      e.altKey &&
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      (e.key === "ArrowUp" || e.key === "ArrowDown")
    ) {
      // from the chats themselves, so it works with the list closed in its sheet
      const order = chatOrder();
      const at = order.findIndex((t) => paneKey(t.kind, t.name) === current);
      const next =
        order[at + (e.key === "ArrowDown" ? 1 : -1)] ?? (at === -1 ? order[0] : undefined);
      if (!next) return;
      e.preventDefault();
      go(next);
    }
  };
</script>

<svelte:window onkeydown={keydown} />

{#snippet list()}
  <ChatList {current} feed={target === "public"} {q} {onjoin} {onnew} />
{/snippet}

{#snippet lead()}
  {#if !viewport.lg}
    <Tooltip text="Chats">
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-sm"
          aria-label="Chats"
          onclick={() => (sheet = true)}><PanelLeftIcon /></Button>
      {/snippet}
    </Tooltip>
  {/if}
{/snippet}

<!-- the whole content area, edge to edge, as a desktop chat app: the list, the conversation and
     its side panel are columns divided by borders -->
<div
  class="flex h-(--content-h) overflow-hidden bg-surface"
  data-chat
  bind:clientWidth={columns.total}>
  {#if viewport.lg}
    {@const { width, min, max, initial } = columns.listColumn}
    <div class="relative flex shrink-0 flex-col border-r" style:width={`${width}px`}>
      {@render list()}
      <ResizeHandle
        bind:value={() => width, (w) => (columns.list = w)}
        {min}
        {max}
        {initial}
        edge="right"
        label="Chat list width"
        onchange={(w) => (chatLayout.listWidth = w)}
        class="absolute inset-y-0 -right-1 z-10 w-2" />
    </div>
  {/if}
  <div class="flex min-w-0 flex-1 flex-col">
    {#if asking}
      <!-- the text keeps the line's width and the button goes under it when both don't fit -->
      <div
        class="flex flex-wrap items-center gap-x-2 gap-y-1 border-b px-3 py-1.5 text-xs text-fg-muted">
        <span class="flex min-w-[min(100%,16rem)] flex-1 items-center gap-2"
          ><BellRingIcon class="size-3.5 shrink-0" />Get a desktop notification for new messages
          while this tab is in the background</span>
        <Button variant="link" class="text-xs" onclick={() => void notifier.ask()}
          >{TURN_ON_NOTIFICATIONS}</Button>
      </div>
    {/if}
    {#if insecure}
      <p class="flex items-center gap-2 border-b px-3 py-1.5 text-xs text-fg-muted">
        <BellOffIcon class="size-3.5 shrink-0" />{DESKTOP_NEEDS_HTTPS}
      </p>
    {/if}
    {#if target === "search"}
      {#key q}
        <SearchResults search={results} {q} {kind} {lead} {onjoin} {onnew} />
      {/key}
    {:else if target === "public"}
      <PublicFeed {lead} />
    {:else if target}
      {#if fromResults}
        <button
          type="button"
          class="flex min-w-0 items-center gap-1.5 border-b px-3 py-1.5 text-left text-xs text-fg-muted hover:text-fg"
          onclick={backToResults}>
          <ArrowLeftIcon class="size-3.5 shrink-0" />
          <span class="truncate">Back to results for "{q}"</span>
        </button>
      {/if}
      {#key `${current} ${at}`}
        <Pane {target} {at} {lead} />
      {/key}
    {:else if viewport.lg}
      <EmptyState icon={MessagesSquareIcon} title="No chat selected" />
    {:else}
      {@render list()}
    {/if}
  </div>
</div>

{#if !viewport.lg}
  <Dialog.Root bind:open={sheet}>
    <Dialog.Portal>
      <Scrim />
      <Dialog.Content
        class="fixed inset-y-0 left-0 z-50 flex w-80 max-w-[90vw] flex-col bg-surface shadow-raised data-[state=closed]:animate-sheet-out data-[state=open]:animate-sheet-in">
        <Dialog.Title class="sr-only">Chats</Dialog.Title>
        {@render list()}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<JoinRoomDialog bind:open={joining} name={joinName} />
<NewMessageDialog bind:open={writing} username={writeTo} />

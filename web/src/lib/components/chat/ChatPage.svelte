<script lang="ts">
  import { afterNavigate, goto } from "$app/navigation";
  import BellRingIcon from "@lucide/svelte/icons/bell-ring";
  import MessagesSquareIcon from "@lucide/svelte/icons/messages-square";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { Dialog } from "bits-ui";
  import { MediaQuery } from "svelte/reactivity";

  import { paneKey } from "#lib/chat/chat.svelte.js";
  import { notifier, notifyOptions } from "#lib/chat/notify.svelte.js";
  import { chatOrder } from "#lib/chat/order.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import { useCrumbs } from "#lib/crumbs.svelte.js";
  import { chatHref, href, roomHref } from "#lib/nav.js";

  import ChatList from "./ChatList.svelte";
  import JoinRoomDialog from "./JoinRoomDialog.svelte";
  import MessageSearch from "./MessageSearch.svelte";
  import NewMessageDialog from "./NewMessageDialog.svelte";
  import Pane from "./Pane.svelte";

  type Props = {
    /** the pane to show, "search" for the message search, null for none */
    target: Target | "search" | null;
    /** a message to open the pane at */
    at: number | null;
    /** the message search's query */
    q: string;
  };

  let { target, at, q }: Props = $props();

  const wide = new MediaQuery("min-width: 1024px");
  let sheet = $state(false);
  let joining = $state(false);
  let joinName = $state("");
  let writing = $state(false);
  let writeTo = $state("");
  let search = $state<HTMLInputElement | null>(null);

  const current = $derived(
    target && target !== "search" ? paneKey(target.kind, target.name) : null
  );
  const asking = $derived(notifier.permission === "default" && notifyOptions().enabled);

  // the breadcrumb names the chat on show
  useCrumbs(() => {
    if (target === "search") return [{ label: "Search messages" }];
    if (!target) return [];
    return [{ label: target.kind === "room" ? `#${target.name}` : target.name }];
  });

  // a narrow screen shows the list in a sheet, which going to a chat closes
  afterNavigate(() => (sheet = false));

  // focus stays where it was, so the keys go on working
  const go = (t: Target) =>
    void goto(t.kind === "user" ? chatHref(t.name) : roomHref(t.name), { reset: false });

  // Ctrl/Cmd+K finds a chat; Alt+Up and Alt+Down move to the chat above or below
  const keydown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (!wide.current) sheet = true;
      queueMicrotask(() => search?.focus());
      return;
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
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
  <ChatList
    bind:search
    {current}
    onjoin={(name) => {
      joinName = name ?? "";
      joining = true;
    }}
    onnew={(name) => {
      writeTo = name ?? "";
      writing = true;
    }} />
{/snippet}

{#snippet lead()}
  {#if !wide.current}
    <Button variant="ghost" size="icon-sm" aria-label="Chats" onclick={() => (sheet = true)}
      ><PanelLeftIcon /></Button>
  {/if}
{/snippet}

<!-- the whole content area, edge to edge, as a desktop chat app: the list, the conversation and
     its side panel are columns divided by borders -->
<div class="flex h-(--content-h) overflow-hidden bg-surface" data-chat>
  {#if wide.current}
    <div class="flex w-72 shrink-0 flex-col border-r">
      {@render list()}
      <a
        href={href("/chat/search")}
        class="flex items-center gap-1.5 border-t px-3 py-2 text-sm text-fg-muted hover:text-fg"
        ><SearchIcon class="size-4" />Search messages</a>
    </div>
  {/if}
  <div class="flex min-w-0 flex-1 flex-col">
    {#if asking}
      <!-- the text keeps the line's width and the button goes under it when both don't fit -->
      <div
        class="flex flex-wrap items-center gap-x-2 gap-y-1 border-b px-3 py-1.5 text-xs text-fg-muted">
        <span class="flex min-w-[min(100%,16rem)] flex-1 items-center gap-2"
          ><BellRingIcon class="size-3.5 shrink-0" />reseek can tell you about new messages while
          this tab is in the background.</span>
        <Button variant="link" class="text-xs" onclick={() => void notifier.ask()}
          >Turn on notifications</Button>
      </div>
    {/if}
    {#if target === "search"}
      <div class="flex flex-col gap-section overflow-y-auto p-4">
        <div class="flex items-center gap-2">
          {@render lead()}
          <h2 class="font-semibold">Search messages</h2>
        </div>
        {#key q}<MessageSearch {q} />{/key}
      </div>
    {:else if target}
      {#key current}
        <Pane {target} {at} {lead} />
      {/key}
    {:else if wide.current}
      <EmptyState icon={MessagesSquareIcon} title="No chat selected" />
    {:else}
      {@render list()}
    {/if}
  </div>
</div>

{#if !wide.current}
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

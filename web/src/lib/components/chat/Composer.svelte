<script module lang="ts">
  /**
   * what was sent from each pane last, which Up in an empty box brings back, kept while the page
   * is open; read only when the key is pressed, so it needn't be reactive
   */
  const recall = new Map<string, string>();
</script>

<script lang="ts">
  import { goto } from "$app/navigation";
  import ClipboardPasteIcon from "@lucide/svelte/icons/clipboard-paste";
  import SendIcon from "@lucide/svelte/icons/send";
  import XIcon from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import { Autosave } from "#lib/autosave.svelte.js";
  import { chat, paneKey } from "#lib/chat/chat.svelte.js";
  import { COMMANDS, commandsFor, parseCommand } from "#lib/chat/commands.js";
  import { emojiFor } from "#lib/chat/emoji.js";
  import type { Message } from "#lib/chat/messages/messages.js";
  import { actionOf } from "#lib/chat/messages/rich.js";
  import type { MessagePane } from "#lib/chat/pane.svelte.js";
  import { chatLayout, roomDrafts } from "#lib/chat/prefs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { chatHref, roomHref, userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { openSearch } from "#lib/search/searches.svelte.js";
  import { links } from "#lib/users/actions.js";
  import { blocking } from "#lib/users/block.svelte.js";

  import LengthCount, { maxLength, nearMax } from "./LengthCount.svelte";

  type Props = {
    pane: MessagePane;
    /** why nothing can be sent now, null when it can */
    disabled: string | null;
    /** names `@` completes */
    names: () => string[];
  };

  let { pane, disabled, names }: Props = $props();

  /** how often a conversation's draft is saved to the daemon while typing, at most */
  const DRAFT_MS = 2_000;
  /** how many completions show at once */
  const SUGGESTIONS = 6;
  /** a paste this long, in characters or lines, gets a note with its size */
  const LONG_PASTE = { chars: 300, lines: 4 };
  /** how much of a message a quote keeps */
  const QUOTED = 100;

  const target = $derived(pane.target);
  const key = $derived(paneKey(target.kind, target.name));
  const max = $derived(maxLength());

  let text = $state("");
  let field = $state<HTMLTextAreaElement | null>(null);
  let caret = $state(0);
  let choice = $state(0);
  /** what the last command said: an error, or the list `/help` asked for */
  let said = $state<{ kind: "error"; text: string } | { kind: "help" } | null>(null);
  /** the size of a long paste, until the text is sent or emptied */
  let pasted = $state<{ chars: number; lines: number } | null>(null);
  let restored = false;

  // the saved draft comes back: a room's from this browser, a conversation's from the daemon
  $effect(() => {
    const draft =
      target.kind === "room" ? (roomDrafts[target.name] ?? "") : pane.conversation?.draft;
    if (draft === undefined || restored) return;
    restored = true;
    untrack(() => {
      if (!text) text = draft;
    });
  });
  // a conversation's draft goes to the daemon; a failed save says nothing: the text stays in the
  // box and its next change saves it again
  const drafts = new Autosave<{ username: string; draft: string }>(
    ({ username, draft }, keepalive) =>
      call(() =>
        api.PUT("/api/v0/conversations/{username}/draft", {
          params: byUser(username),
          body: { draft },
          keepalive
        })
      ),
    DRAFT_MS
  );
  $effect(() => {
    const value = text;
    if (!restored) return;
    untrack(() => {
      if (target.kind === "user") {
        drafts.change({ username: target.name, draft: value.slice(0, 4096) });
      } else if (value) roomDrafts[target.name] = value;
      else delete roomDrafts[target.name];
    });
  });

  type Suggestion = { key: string; label: string; detail?: string; insert: string };

  // what is being typed at the caret: a name after @, an emoji name after :, or a command
  const word = $derived.by(() => {
    const before = text.slice(0, caret);
    const command = /^\/(\w*)$/.exec(before);
    if (command) return { kind: "command" as const, start: 0, typed: command[1] };
    const name = /(?:^|\s)@([^\s@]*)$/.exec(before);
    if (name) return { kind: "name" as const, start: caret - name[1].length - 1, typed: name[1] };
    const emoji = chatLayout.emoji ? /(?:^|\s):([a-z0-9_]{2,})$/i.exec(before) : null;
    if (emoji) {
      return { kind: "emoji" as const, start: caret - emoji[1].length - 1, typed: emoji[1] };
    }
    return null;
  });
  const suggestions = $derived.by((): Suggestion[] => {
    if (!word) return [];
    if (word.kind === "command") {
      return commandsFor(word.typed, target.kind).map((c) => ({
        key: c.name,
        label: `/${c.name}`,
        detail: c.args,
        insert: `/${c.name} `
      }));
    }
    if (word.kind === "emoji") {
      return emojiFor(word.typed, SUGGESTIONS).map((e) => ({
        key: e.name,
        label: `${e.emoji} :${e.name}:`,
        insert: `${e.emoji} `
      }));
    }
    const prefix = word.typed.toLowerCase();
    return names()
      .filter((n) => n.toLowerCase().startsWith(prefix))
      .slice(0, SUGGESTIONS)
      .map((n) => ({ key: n, label: n, insert: `@${n} ` }));
  });

  const complete = (s: Suggestion) => {
    if (!word) return;
    // the word is read before the text changes: it is derived from the text, and an emoji put in
    // its place no longer reads as one
    const start = word.start;
    text = `${text.slice(0, start)}${s.insert}${text.slice(caret)}`;
    caret = start + s.insert.length;
    queueMicrotask(() => field?.setSelectionRange(caret, caret));
  };

  /** Puts a quote of `message` before what is being written, and the caret after it */
  export const quote = (message: Message) => {
    // an action is quoted as what was done, without the `/me` it was sent with
    const line = (actionOf(message.text) ?? message.text).replace(/\s+/g, " ").trim();
    const short = line.length > QUOTED ? `${line.slice(0, QUOTED)}…` : line;
    const head = `[${message.author}] ${short} --> `;
    text = `${head}${text}`;
    caret = head.length;
    queueMicrotask(() => {
      field?.focus();
      field?.setSelectionRange(caret, caret);
    });
  };

  const sendText = async (message: string) => {
    if ([...message].length > max) return;
    text = "";
    pasted = null;
    recall.set(key, message);
    const outcome = await pane.send(message);
    if (!outcome.ok) toast(outcome.error, "error");
  };

  /** Runs a command; what it couldn't do is said under the box */
  const run = async (name: string, who: string, rest: string) => {
    const fail = (why: string) => void (said = { kind: "error", text: why });
    if (name === "help") return void (said = { kind: "help" });
    if (name === "clear") return pane.clearView();
    if (name === "join") {
      const outcome = await chat.join(who, false);
      if (!outcome.ok) return fail(outcome.error);
      return void goto(roomHref(who));
    }
    if (name === "leave") {
      const room = who || target.name;
      const outcome = await chat.leave(room);
      return outcome.ok ? void toast(`Left ${room}`) : fail(outcome.error);
    }
    if (name === "msg") {
      const outcome = await call(() =>
        api.POST("/api/v0/conversations/{username}", { params: byUser(who), body: rest })
      );
      if (!outcome.ok) return fail(outcome.error);
      toast(`Sent to ${who}`, "info", {
        action: { label: "Open", run: () => void goto(chatHref(who)) }
      });
      return;
    }
    if (name === "browse") return void goto(links(who).browse);
    if (name === "whois") return void goto(userHref(who));
    if (name === "block") return blocking.ask(who);
    if (name === "search" || name === "rsearch") {
      const scope =
        name === "rsearch" ? { type: "room" as const, subjects: [target.name] } : undefined;
      openSearch(rest, scope);
    }
  };

  const send = async () => {
    if (disabled) return;
    const parsed = parseCommand(text, target.kind);
    if (parsed.kind === "text") {
      said = null;
      if (parsed.text) await sendText(parsed.text);
      return;
    }
    if (parsed.kind === "error") return void (said = { kind: "error", text: parsed.message });
    said = null;
    text = "";
    pasted = null;
    await run(parsed.name, parsed.target, parsed.text);
  };

  const keydown = (e: KeyboardEvent) => {
    if (suggestions.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const step = e.key === "ArrowDown" ? 1 : -1;
        choice = (choice + step + suggestions.length) % suggestions.length;
        return;
      }
      const picked = suggestions[Math.min(choice, suggestions.length - 1)];
      // Enter on a command already typed in full, such as `/help`, sends it; a name or an emoji
      // typed in full is still completed, and Tab always completes
      const whole =
        word?.kind === "command" && picked.key.toLowerCase() === word.typed.toLowerCase();
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !whole)) {
        e.preventDefault();
        complete(picked);
        return;
      }
    }
    if (e.key === "Escape" && said) {
      e.preventDefault();
      said = null;
      return;
    }
    // Up in an empty box brings back the last message sent here, to correct and send again
    if (e.key === "ArrowUp" && !text && !e.altKey && recall.get(key)) {
      e.preventDefault();
      text = recall.get(key)!;
      caret = text.length;
      return;
    }
    // a private message goes on one line, so Shift+Enter breaks none
    if (e.key === "Enter" && !e.isComposing && (!e.shiftKey || target.kind === "user")) {
      e.preventDefault();
      if (!e.shiftKey) void send();
    }
  };

  const paste = (e: ClipboardEvent) => {
    const clip = e.clipboardData?.getData("text") ?? "";
    const lines = clip.split("\n").length;
    if (clip.length >= LONG_PASTE.chars || lines >= LONG_PASTE.lines) {
      pasted = { chars: [...clip].length, lines };
    }
  };

  const length = $derived([...text].length);
  /** a line break in the box of a private chat, from a paste or a draft, which the daemon replaces */
  const flattened = $derived(target.kind === "user" && /[\r\n]/.test(text));
</script>

<div class="relative flex flex-col gap-1 border-t bg-surface p-2">
  {#if suggestions.length}
    <ul
      role="listbox"
      aria-label={word?.kind === "command"
        ? "Commands"
        : word?.kind === "emoji"
          ? "Emoji"
          : "Usernames"}
      class="absolute bottom-full left-2 z-20 mb-1 min-w-48 rounded-control border bg-overlay p-1 text-sm shadow-raised">
      {#each suggestions as s, i (s.key)}
        <li role="option" aria-selected={i === choice} class="rounded-badge aria-selected:bg-hover">
          <button
            type="button"
            class="flex min-h-control-sm w-full items-center gap-2 rounded-badge px-2 text-left hover:bg-hover/60"
            onclick={() => complete(s)}>
            <span>{s.label}</span>
            {#if s.detail}<span class="text-fg-muted">{s.detail}</span>{/if}
          </button>
        </li>
      {/each}
      {#if word?.kind === "emoji"}
        <li class="mt-1 border-t px-2 pt-1 text-xs text-fg-muted">
          <button type="button" class="hover:text-fg" onclick={() => (chatLayout.emoji = false)}
            >Turn off emoji suggestions</button>
        </li>
      {/if}
    </ul>
  {/if}
  {#if said?.kind === "help"}
    <div
      role="dialog"
      aria-label="Chat commands"
      class="absolute bottom-full left-2 z-20 mb-1 w-[min(28rem,calc(100%-1rem))] rounded-card border bg-overlay p-3 text-sm shadow-raised">
      <div class="mb-2 flex items-center justify-between">
        <span class="font-medium">Commands</span>
        <Tooltip text="Close">
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label="Close"
              onclick={() => (said = null)}><XIcon /></Button>
          {/snippet}
        </Tooltip>
      </div>
      <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
        {#each COMMANDS.filter((c) => !c.roomOnly || target.kind === "room") as c (c.name)}
          <dt class="font-mono text-xs whitespace-nowrap">/{c.name} {c.args}</dt>
          <dd class="text-xs text-fg-muted">{c.about}</dd>
        {/each}
      </dl>
      <p class="mt-2 text-xs text-fg-subtle">Start with // to send a message that begins with /</p>
    </div>
  {/if}
  <div class="flex items-end gap-2">
    <Textarea
      bind:ref={field}
      class="field-sizing-content max-h-48 min-h-10 flex-1 text-sm"
      placeholder={disabled ??
        `Message ${target.kind === "room" ? `#${target.name}` : target.name}`}
      aria-label="Message"
      bind:value={text}
      disabled={!!disabled}
      onkeydown={keydown}
      onpaste={paste}
      oninput={(e) => {
        caret = e.currentTarget.selectionStart ?? text.length;
        choice = 0;
        if (said?.kind === "error") said = null;
        // the note about a paste goes once the box is emptied
        if (!text) pasted = null;
      }}
      onclick={(e) => (caret = e.currentTarget.selectionStart ?? 0)} />
    <Tooltip text="Send">
      {#snippet child({ props })}
        <Button
          {...props}
          size="icon"
          aria-label="Send"
          disabled={!!disabled || !text.trim() || length > max}
          onclick={() => void send()}><SendIcon /></Button>
      {/snippet}
    </Tooltip>
  </div>
  {#if said?.kind === "error" || pasted || flattened || nearMax(length, max)}
    <div class="flex flex-wrap items-center gap-x-3 px-1 text-xs">
      {#if said?.kind === "error"}
        <span class="text-danger" role="alert">{said.text}</span>
      {:else if pasted}
        <span class="flex items-center gap-1 text-fg-muted">
          <ClipboardPasteIcon class="size-3.5" />
          Pasted {plural(pasted.chars, "character", "characters")} on {plural(
            pasted.lines,
            "line",
            "lines"
          )}
        </span>
      {/if}
      {#if flattened}
        <span class="text-fg-muted">Line breaks become spaces in private messages</span>
      {/if}
      {#if nearMax(length, max)}<LengthCount {length} {max} />{/if}
    </div>
  {/if}
</div>

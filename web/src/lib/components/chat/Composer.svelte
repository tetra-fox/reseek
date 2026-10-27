<script lang="ts">
  import SendIcon from "@lucide/svelte/icons/send";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import type { MessagePane } from "#lib/chat/pane.svelte.js";
  import { roomDrafts } from "#lib/chat/prefs.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { call } from "#lib/request.js";
  import { fields } from "#lib/stored.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

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
  /** how many completions show for an `@` */
  const SUGGESTIONS = 6;

  const target = $derived(pane.target);
  const max = $derived.by(() => {
    const configured = fields(fields(app.options).chat).max_message_length;
    return typeof configured === "number" && configured > 0 ? configured : 2000;
  });

  let text = $state("");
  let field = $state<HTMLTextAreaElement | null>(null);
  let caret = $state(0);
  let choice = $state(0);
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
  $effect(() => {
    const value = text;
    if (!restored) return;
    if (target.kind === "room") {
      untrack(() => {
        if (value) roomDrafts[target.name] = value;
        else delete roomDrafts[target.name];
      });
      return;
    }
    // a failed save says nothing: the text stays in the box and its next change saves it again
    const timer = setTimeout(
      () =>
        void call(() =>
          api.PUT("/api/v0/conversations/{username}/draft", {
            params: byUser(target.name),
            body: { draft: value.slice(0, 4096) }
          })
        ),
      DRAFT_MS
    );
    return () => clearTimeout(timer);
  });

  // the word being typed at the caret, when it starts with @
  const word = $derived.by(() => {
    const before = text.slice(0, caret);
    const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
    return match ? { start: caret - match[1].length - 1, prefix: match[1].toLowerCase() } : null;
  });
  const suggestions = $derived(
    word
      ? names()
          .filter((n) => n.toLowerCase().startsWith(word.prefix))
          .slice(0, SUGGESTIONS)
      : []
  );

  const complete = (name: string) => {
    if (!word) return;
    text = `${text.slice(0, word.start)}@${name} ${text.slice(caret)}`;
    caret = word.start + name.length + 2;
    queueMicrotask(() => field?.setSelectionRange(caret, caret));
  };

  const send = async () => {
    const message = text.trim();
    if (!message || disabled || [...message].length > max) return;
    text = "";
    const outcome = await pane.send(message);
    if (!outcome.ok) toast(outcome.error, "error");
  };

  const keydown = (e: KeyboardEvent) => {
    if (suggestions.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const step = e.key === "ArrowDown" ? 1 : -1;
        choice = (choice + step + suggestions.length) % suggestions.length;
        return;
      }
      if (e.key === "Tab" || e.key === "Enter") {
        e.preventDefault();
        complete(suggestions[Math.min(choice, suggestions.length - 1)]);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    }
  };

  const length = $derived([...text].length);
</script>

<div class="relative flex flex-col gap-1 border-t p-2">
  {#if suggestions.length}
    <ul
      role="listbox"
      aria-label="Names"
      class="absolute bottom-full left-2 mb-1 rounded-control border bg-overlay p-1 text-sm shadow-raised">
      {#each suggestions as name, i (name)}
        <li
          role="option"
          aria-selected={i === choice}
          class="cursor-pointer rounded-badge px-2 py-0.5 aria-selected:bg-hover">
          <button type="button" class="w-full text-left" onclick={() => complete(name)}
            >{name}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="flex items-end gap-2">
    <Textarea
      bind:ref={field}
      class="field-sizing-content max-h-48 min-h-10 flex-1 text-sm"
      placeholder={disabled ?? `Message ${target.name}`}
      aria-label="Message"
      bind:value={text}
      disabled={!!disabled}
      onkeydown={keydown}
      oninput={(e) => {
        caret = e.currentTarget.selectionStart ?? text.length;
        choice = 0;
      }}
      onclick={(e) => (caret = e.currentTarget.selectionStart ?? 0)} />
    <Button
      size="icon"
      aria-label="Send"
      disabled={!!disabled || !text.trim() || length > max}
      onclick={() => void send()}><SendIcon /></Button>
  </div>
  {#if length > max * 0.9}
    <p class={["text-right text-xs tabular-nums", length > max ? "text-danger" : "text-fg-muted"]}>
      {length}/{max}
    </p>
  {/if}
</div>

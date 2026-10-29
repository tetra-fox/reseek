<script lang="ts">
  import { goto } from "$app/navigation";
  import CheckCheckIcon from "@lucide/svelte/icons/check-check";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EraserIcon from "@lucide/svelte/icons/eraser";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import HighlighterIcon from "@lucide/svelte/icons/highlighter";
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import UserIcon from "@lucide/svelte/icons/user";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import XIcon from "@lucide/svelte/icons/x";

  import { root, type Schemas } from "#lib/api/client.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import { chatLayout } from "#lib/chat/prefs.svelte.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { links, pin } from "#lib/components/users/actions.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { href, segment, userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { users } from "#lib/users.svelte.js";
  import { BUDDY } from "#lib/wording.js";

  // what can be done to a conversation or a room, for its row's menu and its pane's
  let { target }: { target: Target } = $props();

  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const user = $derived(target.kind === "user");
  const row = $derived(user ? chat.conversations.get(target.name) : chat.rooms.get(target.name));
  const unread = $derived(user ? chat.unreadOf(target.name) : chat.roomUnread(target.name).unread);
  const summary = $derived(user ? users.get(target.name) : undefined);
  const exportUrl = (format: "text" | "json") =>
    user
      ? `${root()}/api/v0/conversations/${segment(target.name)}/export?format=${format}`
      : `${root()}/api/v0/rooms/history/${segment(target.name)}/export?format=${format}`;

  const report = (outcome: { ok: boolean; error?: string }) => {
    if (!outcome.ok) toast(outcome.error ?? "The request failed", "error");
  };
  const markRead = async () =>
    report(user ? await chat.acknowledge(target.name) : await chat.markRoomReadNewest(target.name));
  const notify = async (level: string) =>
    report(await chat.setNotify(target.kind, target.name, level as Schemas["NotifyLevel"]));

  const close = () =>
    confirmation.ask(
      user
        ? {
            title: "Close conversation",
            body: `Close the conversation with ${target.name}? The history is kept, and the conversation reopens when a new message arrives.`,
            confirm: "Close",
            run: () => void chat.close(target.name).then(report)
          }
        : {
            title: "Leave room",
            body: `Leave ${target.name}? Its messages stop arriving until the room is joined again.`,
            confirm: "Leave",
            run: () => void chat.leave(target.name).then(report)
          }
    );
  const clear = () =>
    confirmation.ask({
      title: "Clear history",
      body: user
        ? `Delete every stored message with ${target.name}? The conversation is also closed.`
        : `Delete every stored message in ${target.name}?`,
      confirm: "Delete",
      run: async () =>
        report(user ? await chat.close(target.name, true) : await chat.clearRoom(target.name))
    });
</script>

{#if write && unread}
  <Menu.Item onSelect={() => void markRead()}><CheckCheckIcon />Mark read</Menu.Item>
{/if}
{#if write && row}
  <Menu.Label>Notify</Menu.Label>
  <Menu.RadioGroup value={row.notify} onValueChange={(v) => void notify(v)}>
    <Menu.RadioItem value="all">Every message</Menu.RadioItem>
    <Menu.RadioItem value="mentions">Mentions</Menu.RadioItem>
    <Menu.RadioItem value="none">Nothing</Menu.RadioItem>
  </Menu.RadioGroup>
  <Menu.Separator />
{/if}
{#if !user && row && admin}
  <Menu.Item onSelect={() => goto(href("/system/settings?q=highlight_words"))}
    ><HighlighterIcon />Watched words</Menu.Item>
  <Menu.Separator />
{/if}
<Menu.CheckboxItem checked={chatLayout.emoji} onCheckedChange={(on) => (chatLayout.emoji = on)}
  >Suggest emoji after a colon</Menu.CheckboxItem>
<Menu.Separator />
{#if user}
  <Menu.Item onSelect={() => goto(userHref(target.name))}><UserIcon />View profile</Menu.Item>
  <Menu.Item onSelect={() => goto(links(target.name).browse)}
    ><FolderOpenIcon />Browse shares</Menu.Item>
  {#if write}
    <Menu.Item onSelect={() => void pin(target.name, !summary?.pinned).then(report)}>
      {#if summary?.pinned}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
    </Menu.Item>
  {/if}
{/if}
<Menu.Item onSelect={() => window.location.assign(exportUrl("text"))}
  ><DownloadIcon />Export as text</Menu.Item>
<Menu.Item onSelect={() => window.location.assign(exportUrl("json"))}
  ><DownloadIcon />Export as JSON</Menu.Item>
{#if write}
  <Menu.Separator />
  <Menu.Item onSelect={clear}><EraserIcon />Clear history</Menu.Item>
  {#if row}
    <Menu.Item onSelect={close}>
      {#if user}<XIcon />Close conversation{:else}<LogOutIcon />Leave room{/if}
    </Menu.Item>
  {/if}
{/if}

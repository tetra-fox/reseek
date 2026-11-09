<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { nameColor } from "#lib/users/colors.js";
  import { queueOf } from "#lib/users/groups.js";
  import { nameParts, type UserFound } from "#lib/users/match.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import Marked from "../ui/Marked.svelte";
  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import Presence from "./Presence.svelte";

  let { user, text }: { user: UserFound; text: string } = $props();

  const summary = $derived(user.summary ?? users.get(user.name));
  const queue = $derived(summary ? queueOf(summary) : null);
</script>

<!-- an entry is a picker's option or a result row, which a click picks -->
<Avatar
  name={user.name}
  pictureUrl={pictureOf(user.name, summary)}
  link={false}
  class="size-8 text-xs" />
<span class="flex min-w-0 flex-1 flex-col">
  <span class="truncate text-sm font-medium" style:color={nameColor(user.name)}
    ><Marked parts={nameParts(user.name, text)} /></span>
  {#if summary}
    <span class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-fg-muted">
      <span class="flex items-center gap-1.5">
        <Presence presence={summary.presence} worded />{summary.presence ?? "Unknown"}
      </span>
      {#if queue}<GroupBadge group={queue} queue class="max-w-full" />{/if}
      <Tooltip text={summary.lastSeenAt ? dateTime(summary.lastSeenAt) : undefined} dense>
        {#snippet child({ props })}
          <span {...props} class="min-w-0 truncate">
            {summary.lastSeenAt ? `Last seen ${ago(summary.lastSeenAt, clock.now)}` : "Never seen"}
          </span>
        {/snippet}
      </Tooltip>
    </span>
  {/if}
</span>

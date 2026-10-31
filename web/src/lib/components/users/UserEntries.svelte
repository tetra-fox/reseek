<script lang="ts">
  import UserSearchIcon from "@lucide/svelte/icons/user-search";
  import { Combobox } from "bits-ui";

  import { app } from "#lib/app.svelte.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";
  import { type UserFinder, userValue } from "#lib/users/finder.svelte.js";

  import UserEntry from "./UserEntry.svelte";

  let { finder, text }: { finder: UserFinder; text: string } = $props();
</script>

<!-- entries made anew move the highlight to the first, the closest match, when the users arrive;
     otherwise it stays on the look-up entry that was alone before them -->
{#key finder.found}
  {#each finder.matches as u (u.username)}
    <Suggest.Item value={userValue(u.username)}>
      <UserEntry user={{ name: u.username, summary: u }} {text} />
    </Suggest.Item>
  {/each}
  {#if finder.unknown}
    {#if finder.matches.length}<Combobox.Separator class="-mx-1 my-1 h-px bg-border" />{/if}
    <!-- a look-up holds the dropdown open, where this entry shows it running -->
    <Suggest.Item
      value={userValue(text)}
      disabled={finder.asking || !app.isServerConnected}
      class="flex-col items-stretch gap-0.5 py-1.5">
      <span class="flex items-center gap-2">
        {#if finder.asking}
          <Spinner class="size-4 shrink-0" label={`Looking up ${text}`} />
        {:else}
          <UserSearchIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
        {/if}
        <span class="min-w-0 truncate">{finder.asking ? "Looking up" : "Look up"} {text}</span>
      </span>
      {#if !app.isServerConnected}
        <span class="text-xs text-fg-muted">Connect to the Soulseek server first</span>
      {/if}
    </Suggest.Item>
  {/if}
{/key}

<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";

  import { api, byUser } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";

  // a user's tags: each links to the users with it; typing one and Enter adds it
  type Props = { username: string; tags: string[]; disabled: boolean };
  let { username, tags, disabled }: Props = $props();

  let typed = $state("");

  const save = async (next: string[]) => {
    const outcome = await call(() =>
      api.PUT("/api/v0/users/{username}/tags", { params: byUser(username), body: { tags: next } })
    );
    if (!outcome.ok) toast(outcome.error, "error");
    return outcome.ok;
  };

  const add = async (e: KeyboardEvent) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    const tag = typed.trim().toLowerCase();
    if (!tag || tags.includes(tag)) return void (typed = "");
    if (await save([...tags, tag])) typed = "";
  };
</script>

<div class="flex flex-wrap items-center gap-1">
  {#each tags as tag (tag)}
    <!-- a flex chip, so its link and remove button line up -->
    <Badge tone="outline" class="inline-flex items-center gap-1">
      <a
        href={href(`/users?tag=${encodeURIComponent(tag)}`)}
        class="min-w-0 truncate hover:underline">{tag}</a>
      {#if !disabled}
        <button
          type="button"
          class="shrink-0"
          aria-label={`Remove tag ${tag}`}
          onclick={() => void save(tags.filter((t) => t !== tag))}><XIcon class="size-3" /></button>
      {/if}
    </Badge>
  {/each}
  {#if !disabled}
    <Input
      class="h-control-sm w-32"
      placeholder="Add tag"
      aria-label="Add tag"
      maxlength={32}
      bind:value={typed}
      onkeydown={add} />
  {:else if !tags.length}
    <span class="text-xs text-fg-muted">No tags</span>
  {/if}
</div>

<script lang="ts">
  import Notice from "#lib/components/ui/Notice.svelte";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  import LiveLogs from "./LiveLogs.svelte";
  import LogFiles from "./LogFiles.svelte";

  // the live log and the log files, for administrators
  let { tab }: { tab: "live" | "history" } = $props();
</script>

{#if !session.can("Administrator")}
  <Notice kind="info">Only administrators can view logs.</Notice>
{:else}
  <nav aria-label="Logs" class="flex gap-1">
    {#each [{ id: "live", title: "Live" }, { id: "history", title: "History" }] as t (t.id)}
      <a
        href={href(`/system/logs?tab=${t.id}`)}
        aria-current={tab === t.id ? "page" : undefined}
        class="flex h-control items-center rounded-control px-3 text-sm text-fg-muted hover:bg-hover hover:text-fg aria-[current]:nav-current"
        >{t.title}</a>
    {/each}
  </nav>
  {#if tab === "history"}<LogFiles />{:else}<LiveLogs />{/if}
{/if}

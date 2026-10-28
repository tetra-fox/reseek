<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";

  import { app } from "#lib/app.svelte.js";
  import { href, route, systemTabs } from "#lib/nav.js";

  let { children } = $props();

  const path = $derived(route(page.url).path);

  $effect(() => {
    if (path === "/system" || path === "/system/")
      void goto(href("/system/info"), { replace: true });
  });

  /** A tab that needs attention: Info while a change waits on a restart or reconnect, Shares on a scan */
  const flagged = (tab: string) =>
    (tab === "info" && !!(app.state?.pendingRestart || app.state?.pendingReconnect)) ||
    (tab === "shares" && !!app.state?.shares.scanPending);
</script>

<!-- the padding gives the current tab's ring and shadow room inside the scrolling strip -->
<nav aria-label="System" class="-mx-1 -my-1 flex gap-1 overflow-x-auto px-1 py-1">
  {#each systemTabs as { tab, title } (tab)}
    {@const current = path === `/system/${tab}` || path.startsWith(`/system/${tab}/`)}
    <a
      href={href(`/system/${tab}`)}
      aria-current={current ? "page" : undefined}
      class="flex h-control shrink-0 items-center gap-1.5 rounded-control px-3 text-sm whitespace-nowrap text-fg-muted transition-colors duration-(--motion-fast) hover:bg-hover hover:text-fg aria-[current]:nav-current">
      {#if flagged(tab)}
        <CircleAlertIcon class="size-4 text-warning" aria-label="needs attention" />
      {/if}
      {title}
    </a>
  {/each}
</nav>

{@render children()}

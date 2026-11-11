<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";

  import { app } from "#lib/app.svelte.js";
  import { href, route, systemTabs } from "#lib/nav.js";
  import { leftAt } from "#lib/shell/screens.svelte.js";

  let { children } = $props();

  const path = $derived(route(page.url).path);

  // System opens at the tab it was left at, at first Info
  $effect(() => {
    if (path === "/system" || path === "/system/")
      void goto(href(leftAt("/system") ?? "/system/info"), { replace: true });
  });

  /** A tab that needs attention: Info while a change waits on a restart or reconnect, Shares on a scan */
  const flagged = (tab: string) =>
    (tab === "info" && !!(app.state?.pendingRestart || app.state?.pendingReconnect)) ||
    (tab === "shares" && !!app.state?.shares.scanPending);
</script>

<!-- the tabs wrap onto a second line on a narrow screen, so every one stays in view -->
<nav aria-label="System" class="flex flex-wrap gap-1">
  {#each systemTabs as { tab, title } (tab)}
    {@const current = path === `/system/${tab}` || path.startsWith(`/system/${tab}/`)}
    <a href={href(`/system/${tab}`)} aria-current={current ? "page" : undefined} class="nav-tab">
      {#if flagged(tab)}
        <CircleAlertIcon class="size-4 text-warning" aria-label="Needs attention" />
      {/if}
      {title}
    </a>
  {/each}
</nav>

{@render children()}

<script lang="ts">
  import { afterNavigate, goto, onNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, untrack } from "svelte";
  import { prefersReducedMotion } from "svelte/motion";

  import { activityPrefs } from "#lib/activity/activity.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import ConfirmHost from "#lib/components/ConfirmHost.svelte";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import PlayerBar from "#lib/components/player/PlayerBar.svelte";
  import ActivityDock from "#lib/components/shell/ActivityDock.svelte";
  import CommandPalette from "#lib/components/shell/CommandPalette.svelte";
  import { content } from "#lib/components/shell/content.svelte.js";
  import Footer from "#lib/components/shell/Footer.svelte";
  import Header from "#lib/components/shell/Header.svelte";
  import LostBanner from "#lib/components/shell/LostBanner.svelte";
  import Sidebar from "#lib/components/shell/Sidebar.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import BlockDialog from "#lib/components/users/BlockDialog.svelte";
  import { trail } from "#lib/crumbs.svelte.js";
  import { togglesActivity } from "#lib/keys.js";
  import { live } from "#lib/live/live.svelte.js";
  import {
    baseCrumbs,
    changesScreen,
    fillsContent,
    href,
    redirectFor,
    route,
    titleOf
  } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  // listens for transfer frames from the start, so its notifications follow every screen
  import "#lib/transfers/notices.svelte.js";
  import { SCREEN_FAILED } from "#lib/wording.js";

  let { children } = $props();

  const at = $derived(route(page.url));

  onMount(() => void app.bootstrap());

  // moving to another screen cross-fades the page; a tab, a sub-page or a new query within the
  // same screen changes in place
  onNavigate((navigation) => {
    if (prefersReducedMotion.current || typeof document.startViewTransition !== "function") return;
    const { from, to } = navigation;
    if (!from?.url || !to?.url || !changesScreen(route(from.url).path, route(to.url).path)) return;
    // the content area fades, not the screen in it: its box is the same on every screen, where a
    // padded screen's and a full-height one's differ and would be morphed into each other. It is
    // named only while the transition runs, as a name makes it a stacking context that would put
    // a screen's fixed layers, such as the transfer details drawer, under the header
    const page = content.scroller;
    if (page) page.style.viewTransitionName = "page";
    return new Promise((ready) => {
      const transition = document.startViewTransition(async () => {
        ready();
        await navigation.complete;
      });
      void transition.finished.finally(() => {
        if (page) page.style.viewTransitionName = "";
      });
    });
  });

  // the content area scrolls on its own, so a new screen starts it at the top as a page would.
  // The navigation that first shows a page comes from none, whose url is null
  afterNavigate(({ from, to }) => {
    if (from?.url && to?.url && route(from.url).path !== route(to.url).path) {
      content.scroller?.scrollTo({ top: 0 });
    }
  });

  // screens size what fills the content area from its height, and fixed layers keep clear of
  // the panels below it. The heights go on the document's root, so the toasts and anything else
  // drawn outside the shell, such as a dialog's portal, read the same values
  let contentHeight = $state(0);
  let dockHeight = $state(0);
  let bottomHeight = $state(0);
  let topHeight = $state(0);
  $effect(() => {
    const root = document.documentElement.style;
    if (contentHeight) root.setProperty("--content-h", `${contentHeight}px`);
    if (topHeight) root.setProperty("--top-h", `${topHeight}px`);
    if (bottomHeight) root.setProperty("--bottom-h", `${dockHeight + bottomHeight}px`);
    return () => {
      root.removeProperty("--content-h");
      root.removeProperty("--top-h");
      root.removeProperty("--bottom-h");
    };
  });

  const keydown = (e: KeyboardEvent) => {
    if (!togglesActivity(e) || app.phase !== "app" || app.mode === "agent") return;
    e.preventDefault();
    activityPrefs.open = !activityPrefs.open;
  };

  // a session that is gone goes to the login, which comes back here afterwards
  $effect(() => {
    if (app.phase !== "login") return;
    const here = untrack(() => `${at.path}${at.query.size ? `?${at.query}` : ""}`);
    void goto(href(`/login?to=${encodeURIComponent(here)}`), { replace: true });
  });

  // routes that don't render: unknown ones, and everything but System on a relay agent
  $effect(() => {
    if (app.phase !== "app") return;
    const target = redirectFor(at.path, app.mode === "agent");
    if (target) void goto(href(target), { replace: true });
  });

  // the stream opens once the app is up. untracked, so the state that connecting touches
  // doesn't re-run this
  $effect(() => {
    if (app.phase !== "app") return;
    untrack(() => live.start());
  });
  $effect(() => () => untrack(() => live.stop()));

  // conversations and rooms are followed on every screen, for the unread badge and notifications;
  // a relay agent has no chat
  $effect(() => {
    if (app.phase !== "app" || app.mode === "agent") return;
    untrack(() => chat.start());
    return () => untrack(() => chat.stop());
  });

  // transfers are followed on every screen, for the navigation's badges and their notifications
  $effect(() => {
    if (app.phase !== "app" || app.mode === "agent") return;
    return untrack(() => live.retain("transfers"));
  });

  // the state document by polling while frames don't keep it current
  $effect(() => {
    if (app.phase !== "app") return;
    const ms = live.pollMs;
    if (ms === null) return;
    const timer = setInterval(() => void app.refresh(), ms);
    return () => clearInterval(timer);
  });
</script>

<svelte:head>
  <title
    >{chat.total ? `(${chat.total}) ` : ""}{titleOf([
      ...baseCrumbs(at.path),
      ...trail.page
    ])}</title>
</svelte:head>

<svelte:document onvisibilitychange={() => session.visibility(document.hidden)} />
<svelte:window onkeydown={keydown} />

{#if app.phase === "app"}
  <!-- the window never scrolls: the content area does, and the activity log under it, the
       player and the footer take their room from it rather than covering it -->
  <div class="grid h-svh grid-cols-[minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto]">
    <div class="flex min-h-0">
      <Sidebar />
      <div class="flex min-w-0 flex-1 flex-col">
        <div bind:clientHeight={topHeight}>
          <Header />
          <LostBanner />
        </div>
        <!-- relative, so what is placed absolutely inside, such as a screen reader's hidden label,
             is held and clipped here rather than stretching the window past the shell -->
        <div
          bind:this={content.scroller}
          bind:clientHeight={contentHeight}
          class="relative min-h-0 flex-1 overflow-y-auto"
          data-content>
          <main
            class={[
              "mx-auto flex min-h-full w-full min-w-0 flex-col",
              fillsContent(at.path) ? "h-full" : "max-w-page gap-section p-gutter"
            ]}>
            <svelte:boundary>
              {@render children()}
              {#snippet failed(error, reset)}
                <!-- SvelteKit hands a boundary an App.Error, which hooks.client.ts gives the message -->
                <ErrorSegment
                  caption={SCREEN_FAILED}
                  detail={(error as App.Error).message}
                  retry={reset}
                  retryLabel="Reload" />
              {/snippet}
            </svelte:boundary>
          </main>
        </div>
        <div bind:clientHeight={dockHeight}>
          {#if activityPrefs.open && app.mode !== "agent"}<ActivityDock />{/if}
        </div>
      </div>
    </div>
    <div bind:clientHeight={bottomHeight}>
      <PlayerBar />
      <Footer />
    </div>
  </div>
  <BlockDialog />
  <ConfirmHost />
  <CommandPalette />
{:else if app.phase === "lost"}
  <div class="flex min-h-svh items-center justify-center p-6">
    <ErrorSegment
      caption={app.lostReason}
      detail="reseek did not respond. Check that it is running, then retry."
      retry={() => void app.bootstrap()}
      retryLabel="Retry" />
  </div>
{:else}
  <div class="flex min-h-svh items-center justify-center">
    <Spinner class="size-10" label="Starting" />
  </div>
{/if}

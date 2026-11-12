<script lang="ts">
  import { afterNavigate, goto, onNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, untrack } from "svelte";
  import { prefersReducedMotion } from "svelte/motion";

  import { activityPrefs } from "#lib/activity/activity.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import InspectorPane from "#lib/components/inspector/InspectorPane.svelte";
  import ActivityDock from "#lib/components/shell/ActivityDock.svelte";
  import CommandPalette from "#lib/components/shell/CommandPalette.svelte";
  import ConfirmHost from "#lib/components/shell/ConfirmHost.svelte";
  import Footer from "#lib/components/shell/Footer.svelte";
  import Header from "#lib/components/shell/Header.svelte";
  import LostBanner from "#lib/components/shell/LostBanner.svelte";
  import ShortcutsSheet from "#lib/components/shell/ShortcutsSheet.svelte";
  import Sidebar from "#lib/components/shell/Sidebar.svelte";
  import SendFilesDialog from "#lib/components/transfers/SendFilesDialog.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import BlockDialog from "#lib/components/users/BlockDialog.svelte";
  import { keepsScroll } from "#lib/lists/scroll.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { changesScreen, fillsContent, href, redirectFor, route, titleOf } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { content } from "#lib/shell/content.svelte.js";
  import { baseCrumbs, trail } from "#lib/shell/crumbs.svelte.js";
  import { visited } from "#lib/shell/screens.svelte.js";
  import { APP } from "#lib/ui/bindings.js";
  import { isPressed } from "#lib/ui/keys.js";
  // listens for transfer frames from the start, so its notifications follow every screen
  import "#lib/transfers/notices.svelte.js";
  import "#lib/users/presence.svelte.js";
  import "#lib/jobs/notices.svelte.js";
  import { SCREEN_FAILED } from "#lib/wording.js";

  let { children } = $props();

  const at = $derived(route(page.url));
  /** the content area's place is kept per route path; a new query on the same path keeps it */
  const contentPlace = $derived(`content.${at.path}`);

  onMount(() => void app.bootstrap());

  // moving to another screen cross-fades the page; a tab, a sub-page or a new query within the
  // same screen changes in place
  onNavigate((navigation) => {
    if (prefersReducedMotion.current || typeof document.startViewTransition !== "function") return;
    const { from, to } = navigation;
    if (!from?.url || !to?.url || !changesScreen(route(from.url).path, route(to.url).path)) return;
    // the content area fades, not the screen, whose box differs between padded and full-height
    // screens and would morph. It is named only during the transition: a name makes a stacking
    // context that puts fixed layers, such as the phone's activity sheet, under the header
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

  /** the address that failed to show and what puts its screen back, while the error stands in */
  let broken: { href: string; reset: () => void } | null = null;

  // a screen whose place is in its address comes back to it from the navigation, and a screen
  // that failed is tried again once the user goes to another address
  afterNavigate(({ to }) => {
    if (to?.url) visited(route(to.url).path, route(to.url).query);
    if (!broken || broken.href === to?.url.href) return;
    const { reset } = broken;
    broken = null;
    reset();
  });

  // screens size their content from its height, and fixed layers keep clear of everything under
  // it: the activity log, the panel's inset and the footer, measured as the gap between the
  // content area's bottom and the shell's. Any change to those resizes the content area, which
  // measures again. They go on the root so toasts and dialog portals, drawn outside the shell,
  // read them too
  let shell = $state<HTMLElement>();
  let contentHeight = $state(0);
  $effect(() => {
    const box = content.scroller;
    if (!contentHeight || !shell || !box) return;
    const root = document.documentElement.style;
    const below = shell.getBoundingClientRect().bottom - box.getBoundingClientRect().bottom;
    root.setProperty("--content-h", `${contentHeight}px`);
    root.setProperty("--bottom-h", `${below}px`);
    return () => {
      root.removeProperty("--content-h");
      root.removeProperty("--bottom-h");
    };
  });

  const keydown = (e: KeyboardEvent) => {
    if (!isPressed(APP.activity, e) || app.phase !== "app") return;
    e.preventDefault();
    activityPrefs.open = !activityPrefs.open;
  };

  // a session that is gone goes to the login, which comes back here afterwards
  $effect(() => {
    if (app.phase !== "login") return;
    const here = untrack(() => `${at.path}${at.query.size ? `?${at.query}` : ""}`);
    void goto(href(`/login?to=${encodeURIComponent(here)}`), { replace: true });
  });

  // the root renders nothing of its own
  $effect(() => {
    if (app.phase !== "app") return;
    const target = redirectFor(at.path);
    if (target) void goto(href(target), { replace: true });
  });

  // the stream opens once the app is up. untracked, so the state that connecting touches
  // doesn't re-run this
  $effect(() => {
    if (app.phase !== "app") return;
    untrack(() => live.start());
  });
  $effect(() => () => untrack(() => app.close()));

  // conversations and rooms are followed on every screen, for the unread badge and notifications
  $effect(() => {
    if (app.phase !== "app") return;
    untrack(() => chat.start());
    return () => untrack(() => chat.stop());
  });

  // transfers are followed on every screen, for the navigation's badges and their notifications
  $effect(() => {
    if (app.phase !== "app") return;
    return untrack(() => live.retain("transfers"));
  });

  // polls the state document, the transfer figures and the chats while frames don't keep them
  // current
  $effect(() => {
    if (app.phase !== "app") return;
    const ms = live.pollMs;
    if (ms === null) return;
    const timer = setInterval(() => {
      void app.refresh();
      void chat.load();
    }, ms);
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
  <!-- the window never scrolls: the content area does, and the activity log under it and the
       footer take their room from it rather than covering it. The page may reach under a
       phone's notch and its browser's bottom bar, so the shell keeps clear of what the
       safe-area insets say is covered -->
  <div
    bind:this={shell}
    class="grid h-svh grid-cols-[minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] safe-t safe-r safe-b safe-l">
    <div class="flex min-h-0">
      <Sidebar />
      <!-- the screen's panel, inset from the chrome around it, edge to edge on a phone. The footer
           under it is its bottom inset, so its line sits evenly between the panel and the window -->
      <div
        class="flex min-w-0 flex-1 flex-col overflow-hidden bg-surface md:mt-2 md:mr-2 md:rounded-card md:border">
        <div>
          <Header />
          <LostBanner />
        </div>
        <!-- the inspector stands beside the content area and takes its width from it, as the
             activity log under them takes their height -->
        <div class="flex min-h-0 flex-1">
          <!-- relative, so what is placed absolutely inside, such as a screen reader's hidden
               label, is held and clipped here rather than stretching the window past the shell -->
          <div
            bind:this={content.scroller}
            bind:clientHeight={contentHeight}
            class="relative min-h-0 min-w-0 flex-1 overflow-y-auto"
            data-content
            {@attach keepsScroll(contentPlace)}>
            <main
              class={[
                "mx-auto flex min-h-full w-full min-w-0 flex-col",
                fillsContent(at.path) ? "h-full" : "max-w-page gap-section p-gutter"
              ]}>
              <svelte:boundary onerror={(_, reset) => (broken = { href: page.url.href, reset })}>
                {@render children()}
                {#snippet failed(error, reset)}
                  <!-- SvelteKit hands a boundary an App.Error, which hooks.client.ts gives the message -->
                  <ErrorSegment
                    caption={SCREEN_FAILED}
                    detail={(error as App.Error).message}
                    retry={() => {
                      broken = null;
                      reset();
                    }}
                    retryLabel="Reload" />
                {/snippet}
              </svelte:boundary>
            </main>
          </div>
          <InspectorPane />
        </div>
        {#if activityPrefs.open}<ActivityDock />{/if}
      </div>
    </div>
    <Footer />
  </div>
  <BlockDialog />
  <SendFilesDialog />
  <ConfirmHost />
  <CommandPalette />
  <ShortcutsSheet />
{:else if app.phase === "lost"}
  <div class="flex min-h-svh items-center justify-center p-6">
    <ErrorSegment
      caption={app.lostReason}
      detail={app.lostDetail}
      retry={() => void app.bootstrap()}
      retryLabel="Retry" />
  </div>
{:else}
  <div class="flex min-h-svh items-center justify-center">
    <Spinner class="size-10" label="Starting" />
  </div>
{/if}

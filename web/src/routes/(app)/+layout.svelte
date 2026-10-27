<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import PlayerBar from "#lib/components/player/PlayerBar.svelte";
  import Footer from "#lib/components/shell/Footer.svelte";
  import Header from "#lib/components/shell/Header.svelte";
  import LostBanner from "#lib/components/shell/LostBanner.svelte";
  import Sidebar from "#lib/components/shell/Sidebar.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { live } from "#lib/live/live.svelte.js";
  import { href, redirectFor, route, title } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  let { children } = $props();

  const at = $derived(route(page.url));

  onMount(() => void app.bootstrap());

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
  <title>{title(at.path)}</title>
</svelte:head>

<svelte:document onvisibilitychange={() => session.visibility(document.hidden)} />

{#if app.phase === "app"}
  <div class="flex min-h-svh pb-10">
    <Sidebar />
    <div class="flex min-w-0 flex-1 flex-col">
      <Header />
      <LostBanner />
      <main
        class="mx-auto flex w-full max-w-page min-w-0 flex-1 flex-col gap-section p-gutter pb-16">
        <svelte:boundary>
          {@render children()}
          {#snippet failed(error, reset)}
            <ErrorSegment
              caption={error instanceof Error ? error.message : String(error)}
              retry={reset}
              retryLabel="Reload" />
          {/snippet}
        </svelte:boundary>
      </main>
    </div>
  </div>
  <PlayerBar />
  <Footer />
{:else if app.phase === "lost"}
  <div class="flex min-h-svh items-center justify-center p-6">
    <ErrorSegment
      caption={app.lostReason}
      detail="reseek didn't answer. Check that it's running, then try again."
      retry={() => void app.bootstrap()}
      retryLabel="Retry" />
  </div>
{:else}
  <div class="flex min-h-svh items-center justify-center">
    <Spinner class="size-10" label="Starting" />
  </div>
{/if}

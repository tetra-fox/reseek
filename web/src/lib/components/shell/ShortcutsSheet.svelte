<script lang="ts">
  import { page } from "$app/state";
  import SearchIcon from "@lucide/svelte/icons/search";

  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Keys from "#lib/components/ui/Keys.svelte";
  import { route } from "#lib/nav.js";
  import { shortcuts } from "#lib/shell/shortcuts.svelte.js";
  import { APP, SHEET } from "#lib/ui/bindings.js";
  import { type Binding, isPressed, keepsKey, keysText, layerOpen } from "#lib/ui/keys.js";

  let filter = $state("");
  const path = $derived(route(page.url).path);

  /** what a binding is found by: what it does and its keys, in both spellings */
  const words = (b: Binding) =>
    [b.label, b.typed ?? "", keysText(b, true), keysText(b, false)].join(" ").toLowerCase();

  // the groups of the screen in view come first; a filter keeps the keys whose words have it
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return SHEET.toSorted((a, b) => Number(!!b.here?.(path)) - Number(!!a.here?.(path)))
      .map((g) => ({
        ...g,
        bindings: q ? g.bindings.filter((b) => words(b).includes(q)) : g.bindings
      }))
      .filter((g) => g.bindings.length);
  });

  const keydown = (e: KeyboardEvent) => {
    if (!isPressed(APP.shortcuts, e) || keepsKey(e) || layerOpen()) return;
    e.preventDefault();
    shortcuts.show(document.activeElement);
  };
</script>

<svelte:window onkeydown={keydown} />

<Dialog
  bind:open={
    () => shortcuts.open,
    (open) => {
      shortcuts.open = open;
      // each opening starts with every key, as the palette starts with an empty query
      if (!open) filter = "";
    }
  }
  title="Keyboard shortcuts"
  returnTo={() => shortcuts.from}
  class="w-[min(48rem,calc(100vw-2rem))]">
  <Input
    type="search"
    icon={SearchIcon}
    placeholder="Filter shortcuts"
    aria-label="Filter shortcuts"
    bind:value={filter} />
  {#if shown.length}
    <div class="gap-section md:columns-2">
      {#each shown as g (g.title)}
        {@const id = `keys-${g.title.toLowerCase().replaceAll(" ", "-")}`}
        <section class="mb-group break-inside-avoid" aria-labelledby={id}>
          <h3 {id} class="pb-1 text-xs font-medium text-fg-muted">{g.title}</h3>
          <dl class="flex flex-col">
            {#each g.bindings as b (b.label)}
              <div
                class="flex items-start justify-between gap-inline border-b py-1.5 last:border-b-0">
                <dt class="text-sm">{b.label}</dt>
                <dd class="shrink-0 text-right text-xs text-fg-muted">
                  <Keys binding={b} class="justify-end" />
                </dd>
              </div>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
  {:else}
    <p class="py-6 text-center text-sm text-fg-muted">No shortcuts match</p>
  {/if}
</Dialog>

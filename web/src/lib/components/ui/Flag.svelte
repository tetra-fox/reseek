<script lang="ts" module>
  // the package's 4:3 flags, one image file per two-letter code, which the browser fetches only for
  // the countries it shows rather than every page loading them inlined in its script. `xx` is left
  // out, since it is the package's placeholder box. the files with longer names are subdivisions
  // and unions, which no country code from the server names.
  const FILES = import.meta.glob<string>(
    ["/node_modules/flag-icons/flags/4x3/??.svg", "!/node_modules/flag-icons/flags/4x3/xx.svg"],
    { eager: true, query: "?url&no-inline", import: "default" }
  );
  const FLAGS = new Map(
    Object.entries(FILES).map(([path, url]) => [path.slice(-6, -4).toUpperCase(), url])
  );
  const NAMES = new Intl.DisplayNames(undefined, { type: "region" });
</script>

<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a country as an image of its flag, since Windows has no flag emoji, with its name as the label
  // and tooltip; nothing for a code with no flag
  let { code }: { code: string | null | undefined } = $props();

  const country = $derived(code?.toUpperCase() ?? "");
  const url = $derived(FLAGS.get(country));
</script>

{#if url}
  {@const name = NAMES.of(country)}
  <Tooltip text={name} dense>
    {#snippet child({ props })}
      <img
        {...props}
        src={url}
        alt={name}
        draggable="false"
        class="h-3 w-4 shrink-0 rounded-[1px] object-cover outline -outline-offset-1 outline-fg/15" />
    {/snippet}
  </Tooltip>
{/if}

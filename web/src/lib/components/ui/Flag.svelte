<script lang="ts" module>
  // image files, fetched per country shown, rather than inlined into the script every page loads.
  // xx is the package's placeholder box; longer names are regions no country code can be
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
  // a country as an image of its flag, since Windows has no flag emoji, with its name as the label
  // and tooltip; nothing for a code with no flag
  let { code }: { code: string | null | undefined } = $props();

  const country = $derived(code?.toUpperCase() ?? "");
  const url = $derived(FLAGS.get(country));
</script>

{#if url}
  {@const name = NAMES.of(country)}
  <img
    src={url}
    alt={name}
    title={name}
    draggable="false"
    class="h-3 w-4 shrink-0 rounded-[1px] object-cover outline -outline-offset-1 outline-fg/15" />
{/if}

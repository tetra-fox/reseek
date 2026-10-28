<script lang="ts">
  import { root } from "#lib/api/client.js";
  import { nameHue } from "#lib/colours.js";
  import { cn } from "#lib/utils.js";

  type Props = {
    name: string;
    /** the picture's path under the daemon, null for the initial-letter avatar */
    pictureUrl?: string | null;
    /** a picture held in the page, such as one picked and not yet uploaded; wins over `pictureUrl` */
    src?: string | null;
    class?: string;
  };

  let { name, pictureUrl, src, class: className = "size-8" }: Props = $props();

  // the hue of the name's colour, so a face and a name match
  const hue = $derived(nameHue(name));
</script>

{#if src || pictureUrl}
  <img
    src={src || `${root()}${pictureUrl}`}
    alt=""
    class={cn("shrink-0 rounded-full object-cover", className)} />
{:else}
  <span
    aria-hidden="true"
    class={cn(
      "flex shrink-0 items-center justify-center rounded-full text-sm font-semibold text-accent-fg",
      className
    )}
    style:background-color={`oklch(0.6 0.12 ${hue})`}>
    {[...name][0]?.toUpperCase() ?? "?"}
  </span>
{/if}

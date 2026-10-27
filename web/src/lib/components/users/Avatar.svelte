<script lang="ts">
  import { root } from "#lib/api/client.js";
  import { cn } from "#lib/utils.js";

  type Props = {
    name: string;
    /** the picture's path under the daemon, null for the initial-letter avatar */
    pictureUrl?: string | null;
    class?: string;
  };

  let { name, pictureUrl, class: className = "size-8" }: Props = $props();

  // the same name always gets the same hue
  const hue = $derived([...name].reduce((h, c) => (h * 31 + c.codePointAt(0)!) % 360, 7));
</script>

{#if pictureUrl}
  <img
    src={`${root()}${pictureUrl}`}
    alt=""
    class={cn("shrink-0 rounded-full object-cover", className)} />
{:else}
  <span
    aria-hidden="true"
    class={cn(
      "text-white flex shrink-0 items-center justify-center rounded-full text-sm font-semibold",
      className
    )}
    style:background-color={`oklch(0.6 0.12 ${hue})`}>
    {[...name][0]?.toUpperCase() ?? "?"}
  </span>
{/if}

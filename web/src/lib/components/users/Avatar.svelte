<script lang="ts">
  import { Avatar } from "bits-ui";

  import { root } from "#lib/api/client.js";
  import { SERVER } from "#lib/chat/notices.js";
  import { userHref } from "#lib/nav.js";
  import { cn } from "#lib/ui/cn.js";
  import { nameHue } from "#lib/users/colors.js";

  type Props = {
    name: string;
    /** the picture's path under the daemon, null for the initial-letter avatar */
    pictureUrl?: string | null;
    /** a picture held in the page, such as one picked and not yet uploaded; wins over `pictureUrl` */
    src?: string | null;
    /** what the picture shows, for where the name is not beside it; empty leaves it unread */
    alt?: string;
    /**
     * a click goes to the user's profile, as their name does; false inside a control, a picker's
     * option or on the profile itself
     */
    link?: boolean;
    class?: string;
  };

  let {
    name,
    pictureUrl,
    src,
    alt = "",
    link = true,
    class: className = "size-8"
  }: Props = $props();

  // the hue of the name's color, so a face and a name match
  const hue = $derived(nameHue(name));
</script>

<!-- the initial shows until the picture has loaded, and stays when it fails to -->
{#snippet face()}
  <Avatar.Root>
    {#snippet child({ props })}
      <span
        {...props}
        class={cn(
          "flex shrink-0 overflow-hidden rounded-full text-sm font-medium text-accent-fg",
          className
        )}>
        {#if src || pictureUrl}
          <Avatar.Image
            src={src || `${root()}${pictureUrl}`}
            {alt}
            class="size-full object-cover" />
        {/if}
        <Avatar.Fallback
          class="flex size-full items-center justify-center"
          style={`background-color: oklch(0.6 0.12 ${hue})`}
          role={alt ? "img" : undefined}
          aria-label={alt || undefined}
          aria-hidden={alt ? undefined : "true"}>
          {[...name][0]?.toUpperCase() ?? "?"}
        </Avatar.Fallback>
      </span>
    {/snippet}
  </Avatar.Root>
{/snippet}

<!-- the link is for the pointer only: the name beside the face, or the row it is in, is the
     keyboard's way to the profile, so the face adds no tab stop. The server's notices have no
     profile -->
{#if link && name !== SERVER}
  <a href={userHref(name)} tabindex="-1" aria-hidden="true" class="contents">{@render face()}</a>
{:else}
  {@render face()}
{/if}

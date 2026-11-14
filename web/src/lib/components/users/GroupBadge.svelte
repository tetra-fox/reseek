<script lang="ts">
  import type { Snippet } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { NOT_CONFIGURED } from "#lib/wording.js";

  import GroupDot from "./GroupDot.svelte";

  // a group a user is in, with its color; one the configuration no longer has is dimmed
  type Props = {
    group: Schemas["UserGroup"];
    /** the user's uploads wait in its queue */
    queue?: boolean;
    /** where the name links to */
    href?: string;
    class?: string;
    /** after the name, such as a button that takes the user out of it */
    children?: Snippet;
    /** its tooltip waits for its row to wake, as `Tooltip`'s `dormant` */
    dormant?: boolean;
  };

  let { group, queue = false, href, class: className, children, dormant = false }: Props = $props();

  const tip = $derived(
    !group.defined ? NOT_CONFIGURED : queue ? `Queue: ${group.name}` : undefined
  );
</script>

<Tooltip text={tip} dense {dormant}>
  {#snippet child({ props })}
    <Badge
      {...props}
      tone="outline"
      class={[
        "inline-flex items-center gap-1",
        !group.defined && "border-dashed opacity-60",
        className
      ]}>
      <GroupDot color={group.color} />
      {#if href}
        <a {href} class="min-w-0 truncate hover:underline">{group.name}</a>
      {:else}
        <span class="min-w-0 truncate">{group.name}</span>
      {/if}
      {@render children?.()}
    </Badge>
  {/snippet}
</Tooltip>

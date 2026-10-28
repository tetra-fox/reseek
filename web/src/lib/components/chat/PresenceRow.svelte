<script lang="ts">
  import LogInIcon from "@lucide/svelte/icons/log-in";
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import MoonIcon from "@lucide/svelte/icons/moon";
  import ShieldIcon from "@lucide/svelte/icons/shield";
  import SunIcon from "@lucide/svelte/icons/sun";

  import { type Burst, burstLine } from "#lib/chat/presence.js";
  import Username from "#lib/components/users/Username.svelte";
  import { formatDate, shortTime } from "#lib/format.js";

  // members coming and going, one muted line for a burst of the same change
  let { burst }: { burst: Burst } = $props();

  const line = $derived(burstLine(burst));
  const Icon = $derived(
    {
      joined: LogInIcon,
      added: LogInIcon,
      left: LogOutIcon,
      removed: LogOutIcon,
      away: MoonIcon,
      returned: SunIcon,
      operator: ShieldIcon,
      notOperator: ShieldIcon
    }[burst.kind]
  );
</script>

<p role="note" class="flex items-center justify-center gap-1.5 px-3 py-1 text-xs text-fg-muted">
  <Icon class="size-3 shrink-0 text-fg-subtle" aria-hidden="true" />
  <span class="min-w-0 truncate">
    {#each line.shown as name, i (name)}{#if i > 0}{i === line.shown.length - 1 && !line.others
          ? " and "
          : ", "}{/if}<Username {name} card class="font-normal" />{/each}{#if line.others}
      and {line.others} others{/if}
    {line.verb}
  </span>
  <time class="shrink-0 text-fg-subtle" datetime={burst.at} title={formatDate(burst.at)}
    >{shortTime(burst.at)}</time>
</p>

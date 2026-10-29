<script lang="ts">
  import LogInIcon from "@lucide/svelte/icons/log-in";
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import MoonIcon from "@lucide/svelte/icons/moon";
  import ShieldIcon from "@lucide/svelte/icons/shield";
  import SunIcon from "@lucide/svelte/icons/sun";

  import { type Burst, burstLine } from "#lib/chat/rooms/presence.js";
  import LogTime from "#lib/components/ui/LogTime.svelte";
  import Username from "#lib/components/users/Username.svelte";

  // the cells of one line of a room's activity: a burst of the same change by its members
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

<LogTime at={burst.at} />
<span role="gridcell" class="flex justify-center pt-1">
  <Icon class="size-3.5 text-fg-muted" aria-hidden="true" />
</span>
<span role="gridcell" class="min-w-0 wrap-anywhere">
  {#each line.shown as name, i (name)}{#if i > 0}{i === line.shown.length - 1 && !line.others
        ? " and "
        : ", "}{/if}<Username {name} card />{/each}{#if line.others}
    and <span title={burst.names.slice(line.shown.length).join(", ")}>{line.others} others</span
    >{/if}
  <span class="text-fg-muted">{line.verb}</span>
</span>
